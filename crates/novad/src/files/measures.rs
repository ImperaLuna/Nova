//! Measuring jobs (`files_measure`): the Properties window's size of a
//! folder or a selection, counted on a thread of its own so a big folder
//! never holds up anything else, and reported to the client that asked
//! every [`FILES_MEASURE_PROGRESS_MS`] and once more when done.
//!
//! A job walks its items without following links below them (a link
//! counts as a file of its own size; an item that is a link is followed,
//! as the list shows it), counts a file with several names once, stays on each item's drive (as
//! `du -x`: a drive mounted inside, or `/proc` under `/`, isn't counted),
//! and skips folders it can't read.
//! It checks for a stop before each entry. Every report is sent while the
//! job is still registered, checked under the jobs lock, so nothing
//! arrives after a cancel, the client's hang-up or the module stopping;
//! the last one unregisters it in the same step.

use crate::files::folder::disk_bytes;
use crate::files::views::checked_path;
use crate::hub::{ClientId, Hub};
use crate::lock;
use nova_protocol::{Event, FILES_MAX_MEASURES, FILES_MEASURE_PROGRESS_MS, FILES_TRANSFER_MAX_SOURCES, Measured};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PROGRESS_EVERY: Duration = Duration::from_millis(FILES_MEASURE_PROGRESS_MS);

/// A job, named by its client and the client's number for it.
type Key = (ClientId, u32);

/// The running jobs, each with its stop flag; `None` once the module
/// stopped, so a `files_measure` on its way can't start one after that.
type Jobs = Arc<Mutex<Option<HashMap<Key, Arc<AtomicBool>>>>>;

/// The measuring jobs clients run.
pub struct Measures {
    hub: Arc<Hub>,
    jobs: Jobs,
}

impl Measures {
    pub fn new(hub: Arc<Hub>) -> Self {
        Self { hub, jobs: Arc::new(Mutex::new(Some(HashMap::new()))) }
    }

    /// Starts `client`'s job `job`, measuring `paths`. Doesn't touch the
    /// disk: the counts follow as events.
    pub fn start(&self, client: ClientId, job: u32, paths: &[String]) -> Result<(), String> {
        if paths.is_empty() {
            return Err("Nothing to measure".into());
        }
        if paths.len() > FILES_TRANSFER_MAX_SOURCES {
            return Err(format!("At most {FILES_TRANSFER_MAX_SOURCES} items can be measured at once"));
        }
        let paths = paths.iter().map(|path| checked_path(path)).collect::<Result<Vec<_>, _>>()?;
        let key = (client, job);
        let stopped = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = lock(&self.jobs);
            let Some(running) = jobs.as_mut() else { return Err("Nova Files is off".into()) };
            if running.contains_key(&key) {
                return Err(format!("Measuring job {job} is already running"));
            }
            if running.keys().filter(|(owner, _)| *owner == client).count() >= FILES_MAX_MEASURES {
                return Err(format!("At most {FILES_MAX_MEASURES} sizes can be counted at once"));
            }
            running.insert(key, Arc::clone(&stopped));
        }
        let mut run = Run {
            key,
            stopped: Arc::clone(&stopped),
            hub: Arc::clone(&self.hub),
            jobs: Arc::clone(&self.jobs),
            counted: Measured { job, files: 0, folders: 0, bytes: 0, disk_bytes: 0, done: false },
            last_report: Instant::now(),
            linked: HashSet::new(),
        };
        let spawned = std::thread::Builder::new().name("novad-files-measure".into()).spawn(move || run.run(&paths));
        spawned.map(drop).map_err(|e| {
            self.forget(|found| *found == key);
            format!("Can't start counting: {e}")
        })
    }

    /// Stops `client`'s job `job`, which sends nothing more.
    pub fn cancel(&self, client: ClientId, job: u32) {
        self.forget(|key| *key == (client, job));
    }

    /// Stops every job of `client`, which hung up.
    pub fn client_gone(&self, client: ClientId) {
        self.forget(|(owner, _)| *owner == client);
    }

    /// Stops every job, for good.
    pub fn stop(&self) {
        for (_, stopped) in lock(&self.jobs).take().into_iter().flatten() {
            stopped.store(true, Ordering::Relaxed);
        }
    }

    fn forget(&self, which: impl Fn(&Key) -> bool) {
        let mut jobs = lock(&self.jobs);
        let Some(running) = jobs.as_mut() else { return };
        running.retain(|key, stopped| {
            let keep = !which(key);
            if !keep {
                stopped.store(true, Ordering::Relaxed);
            }
            keep
        });
    }
}

/// One job, on its own thread.
struct Run {
    key: Key,
    stopped: Arc<AtomicBool>,
    hub: Arc<Hub>,
    jobs: Jobs,
    counted: Measured,
    last_report: Instant,
    /// The files with more than one name already counted.
    linked: HashSet<(u64, u64)>,
}

impl Run {
    fn run(&mut self, paths: &[PathBuf]) {
        for path in paths {
            if self.stopped() {
                return;
            }
            // An item that's gone or can't be read adds nothing. The item
            // itself is followed when it's a link, as the list shows it.
            if let Ok(meta) = fs::metadata(path).or_else(|_| fs::symlink_metadata(path)) {
                self.add(&meta);
                if meta.is_dir() && !self.walk(path, meta.dev()) {
                    return;
                }
            }
        }
        self.counted.done = true;
        self.report();
    }

    /// Counts what `top` holds on drive `device`, one folder at a time from
    /// a list of those still to read (so a deep tree holds no open folders
    /// and no deep stack); false once the job was stopped.
    fn walk(&mut self, top: &Path, device: u64) -> bool {
        let mut folders = vec![top.to_path_buf()];
        while let Some(folder) = folders.pop() {
            if self.stopped() {
                return false;
            }
            let Ok(entries) = fs::read_dir(&folder) else { continue };
            for entry in entries {
                if self.stopped() {
                    return false;
                }
                let Ok(entry) = entry else { continue };
                let path = entry.path();
                let Ok(meta) = fs::symlink_metadata(&path) else { continue };
                if meta.dev() != device {
                    continue;
                }
                if meta.is_dir() {
                    self.counted.folders += 1;
                    folders.push(path);
                } else {
                    self.counted.files += 1;
                }
                self.add(&meta);
                if self.last_report.elapsed() >= PROGRESS_EVERY {
                    self.report();
                }
            }
        }
        !self.stopped()
    }

    /// Adds an item's bytes. A file with more than one name (a hard link)
    /// adds them once, as `du` does, however many of its names are found.
    fn add(&mut self, meta: &fs::Metadata) {
        if meta.nlink() > 1 && !meta.is_dir() && !self.linked.insert((meta.dev(), meta.ino())) {
            return;
        }
        if !meta.is_dir() {
            self.counted.bytes += meta.len();
        }
        self.counted.disk_bytes += disk_bytes(meta);
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    /// Sends the counts so far, while the job is still registered; the last
    /// report unregisters it.
    fn report(&mut self) {
        self.last_report = Instant::now();
        let mut jobs = lock(&self.jobs);
        let Some(running) = jobs.as_mut() else { return };
        if !running.get(&self.key).is_some_and(|shown| Arc::ptr_eq(shown, &self.stopped)) {
            return;
        }
        self.hub.send_to(self.key.0, &Event::FilesMeasured(self.counted));
        if self.counted.done {
            running.remove(&self.key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{ClientQueue, Outgoing, next_named};
    use nova_protocol::Topic;
    use nova_test_support::TempDir;
    use serde_json::{Value, json};
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc::Receiver;

    const WAIT: Duration = Duration::from_secs(5);

    fn client(hub: &Hub) -> (ClientId, Receiver<Outgoing>, UnixStream) {
        let (stream, ours) = UnixStream::pair().unwrap();
        let (queue, outgoing) = ClientQueue::new(&ours).unwrap();
        let client = hub.register(queue);
        hub.subscribe(client, Topic::Files);
        (client, outgoing, stream)
    }

    fn wire(path: &Path) -> String {
        nova_protocol::wire_text(path)
    }

    /// The job's last report.
    fn finished(events: &Receiver<Outgoing>) -> Value {
        loop {
            let measured = next_named(events, "files_measured", WAIT);
            if measured["done"] == json!(true) {
                return measured;
            }
        }
    }

    #[test]
    fn a_folder_counts_everything_inside_it_but_itself() {
        let dir = TempDir::new("measure-folder");
        let photos = dir.join("Photos");
        fs::create_dir_all(photos.join("2026/Summer")).unwrap();
        fs::write(photos.join("cover.png"), vec![0u8; 1000]).unwrap();
        fs::write(photos.join("2026/Summer/beach.jpg"), vec![0u8; 2500]).unwrap();
        std::os::unix::fs::symlink("/nowhere", photos.join("broken")).unwrap();
        let hub = Arc::new(Hub::default());
        let measures = Measures::new(Arc::clone(&hub));
        let (id, events, _stream) = client(&hub);
        measures.start(id, 7, &[wire(&photos)]).unwrap();
        let measured = finished(&events);
        assert_eq!(measured["job"], json!(7));
        assert_eq!(measured["files"], json!(3), "two files and a link");
        assert_eq!(measured["folders"], json!(2));
        let bytes = measured["bytes"].as_u64().unwrap();
        assert_eq!(bytes, 3500 + "/nowhere".len() as u64, "a link counts as itself");
        assert!(measured["disk_bytes"].as_u64().unwrap() > 0);
    }

    #[test]
    fn a_linked_folder_counts_what_it_points_to_and_hard_links_count_once() {
        let dir = TempDir::new("measure-links");
        fs::create_dir(dir.join("Photos")).unwrap();
        fs::write(dir.join("Photos/a.jpg"), vec![0u8; 400]).unwrap();
        fs::hard_link(dir.join("Photos/a.jpg"), dir.join("Photos/b.jpg")).unwrap();
        std::os::unix::fs::symlink("Photos", dir.join("Pictures")).unwrap();
        let hub = Arc::new(Hub::default());
        let measures = Measures::new(Arc::clone(&hub));
        let (id, events, _stream) = client(&hub);
        measures.start(id, 2, &[wire(&dir.join("Pictures"))]).unwrap();
        let measured = finished(&events);
        assert_eq!(measured["files"], json!(2), "both names are listed");
        assert_eq!(measured["bytes"], json!(400), "one file's bytes");
    }

    #[test]
    fn a_selection_adds_up_its_items() {
        let dir = TempDir::new("measure-selection");
        fs::write(dir.join("a.txt"), b"12345").unwrap();
        fs::create_dir(dir.join("Notes")).unwrap();
        fs::write(dir.join("Notes/b.txt"), b"123").unwrap();
        let hub = Arc::new(Hub::default());
        let measures = Measures::new(Arc::clone(&hub));
        let (id, events, _stream) = client(&hub);
        measures.start(id, 1, &[wire(&dir.join("a.txt")), wire(&dir.join("Notes")), wire(&dir.join("gone"))]).unwrap();
        let measured = finished(&events);
        assert_eq!((measured["files"].clone(), measured["folders"].clone()), (json!(1), json!(0)));
        assert_eq!(measured["bytes"], json!(8));
    }

    #[test]
    fn a_cancelled_job_frees_its_number() {
        let dir = TempDir::new("measure-cancel");
        let hub = Arc::new(Hub::default());
        let measures = Measures::new(Arc::clone(&hub));
        let (id, events, _stream) = client(&hub);
        measures.start(id, 3, &[wire(&dir)]).unwrap();
        measures.cancel(id, 3);
        measures.start(id, 3, &[wire(&dir)]).unwrap();
        assert_eq!(finished(&events)["job"], json!(3));
    }

    #[test]
    fn bad_requests_are_refused() {
        let hub = Arc::new(Hub::default());
        let measures = Measures::new(Arc::clone(&hub));
        let (id, _events, _stream) = client(&hub);
        assert!(measures.start(id, 1, &[]).is_err());
        assert!(measures.start(id, 1, &["relative".into()]).is_err());
        measures.stop();
        assert!(measures.start(id, 1, &["/".into()]).unwrap_err().contains("off"));
    }
}
