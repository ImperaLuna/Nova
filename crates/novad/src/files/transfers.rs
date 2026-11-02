//! Copy and move jobs (`files_transfer`): each client's running jobs, one
//! thread each, reporting to the client that started them.
//!
//! A job first checks its items and measures them ([`copying::measure`]),
//! then places them one by one, depth first. An item copied into the
//! folder it's already in gets a free numbered name; one moved there stays
//! put. A name that's taken asks the client with a [`Clash`] and waits for
//! `files_transfer_answer` (or the same answer given to all clashes of its
//! kind earlier). A file replaces a file; a folder merges into a folder,
//! asking about each clash inside. Files are written under a temporary
//! name and renamed into place once complete, so a cancelled or failed
//! copy leaves no half file, and a replaced file is swapped in one step.
//! A move is a rename when it stays on one file system, and a copy and
//! delete when it doesn't.
//!
//! The job checks for a cancel before each item and between chunks of a
//! file. Every event is sent while the job is still registered, checked
//! under the jobs lock, so nothing arrives after it ended, was cancelled
//! by the client's hang-up or the module stopped; the last one (done,
//! cancelled or failed) unregisters it in the same step.

use crate::files::copying::{self, Aside, Clashing, Measure, Part};
use crate::files::views::checked_path;
use crate::hub::{ClientId, Hub};
use crate::lock;
use crate::logger::Logger;
use nova_protocol::{
    Clash, ClashAnswer, Event, FILES_MAX_TRANSFERS, FILES_TRANSFER_MAX_SOURCES, FILES_TRANSFER_PROGRESS_MS, Transfer,
    TransferMode, TransferState,
};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const LOG_SOURCE: &str = "files";
const PROGRESS_EVERY: Duration = Duration::from_millis(FILES_TRANSFER_PROGRESS_MS);

/// A job, named by its client and the client's number for it.
type Key = (ClientId, u32);

/// The running jobs; `None` once the module stopped, so a `files_transfer`
/// already on its way can't start one after that.
type Jobs = Arc<Mutex<Option<HashMap<Key, Arc<Job>>>>>;

/// The copy and move jobs clients run.
pub struct Transfers {
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    jobs: Jobs,
}

struct Job {
    key: Key,
    mode: TransferMode,
    /// How many items it was given, and the first one's name.
    items: u32,
    name: String,
    control: Mutex<Control>,
    /// Signalled when `control` changes.
    changed: Condvar,
}

/// What the client tells a job while it runs.
#[derive(Default)]
struct Control {
    stopped: bool,
    /// The clash it waits on: its id, and whether it's a folder clash.
    asking: Option<(u32, bool)>,
    /// The answer to it, and whether it goes to the rest of its kind.
    answer: Option<(ClashAnswer, bool)>,
}

impl Transfers {
    pub fn new(hub: Arc<Hub>, logger: Arc<Logger>) -> Self {
        Self { hub, logger, jobs: Arc::new(Mutex::new(Some(HashMap::new()))) }
    }

    /// Starts `client`'s job `job`, copying or moving `sources` into
    /// `folder`. Doesn't touch the disk: how it goes follows as events.
    pub fn start(
        &self,
        client: ClientId,
        job: u32,
        mode: TransferMode,
        sources: &[String],
        folder: &str,
    ) -> Result<(), String> {
        if sources.is_empty() {
            return Err("Nothing to copy".into());
        }
        if sources.len() > FILES_TRANSFER_MAX_SOURCES {
            return Err(format!("At most {FILES_TRANSFER_MAX_SOURCES} items can be copied at once"));
        }
        let sources = sources.iter().map(|source| checked_path(source)).collect::<Result<Vec<_>, _>>()?;
        let folder = checked_path(folder)?;
        let name = sources[0].file_name().map_or_else(|| "/".into(), nova_protocol::wire_text);
        let key = (client, job);
        let mut jobs = lock(&self.jobs);
        let Some(running) = jobs.as_mut() else { return Err("Nova Files is off".into()) };
        if running.contains_key(&key) {
            return Err(format!("Job {job} is already running"));
        }
        if running.keys().filter(|(owner, _)| *owner == client).count() >= FILES_MAX_TRANSFERS {
            return Err(format!("At most {FILES_MAX_TRANSFERS} copies can run at once"));
        }
        let items = u32::try_from(sources.len()).unwrap_or(u32::MAX);
        let started = Arc::new(Job { key, mode, items, name, control: Mutex::default(), changed: Condvar::new() });
        running.insert(key, Arc::clone(&started));
        drop(jobs);
        let run = Run {
            job: started,
            hub: Arc::clone(&self.hub),
            logger: Arc::clone(&self.logger),
            jobs: Arc::clone(&self.jobs),
            sources,
            folder,
            done_bytes: 0,
            total: Measure::default(),
            remembered: [None; 2],
            next_clash: 0,
            last_report: Instant::now(),
            copying_moved: false,
        };
        let spawned = std::thread::Builder::new().name("novad-files-copy".into()).spawn(move || run.run());
        spawned.map(drop).map_err(|e| {
            self.forget(|key| *key == (client, job));
            format!("Can't start copying: {e}")
        })
    }

    /// Stops `client`'s job `job`, which then reports it was cancelled.
    /// One that already ended is left alone.
    pub fn cancel(&self, client: ClientId, job: u32) {
        if let Some(job) = lock(&self.jobs).as_ref().and_then(|jobs| jobs.get(&(client, job)).cloned()) {
            job.stop();
        }
    }

    /// Answers the clash `clash` that `client`'s job `job` asks about.
    pub fn answer(&self, client: ClientId, job: u32, clash: u32, answer: ClashAnswer, all: bool) -> Result<(), String> {
        let found = lock(&self.jobs).as_ref().and_then(|jobs| jobs.get(&(client, job)).cloned());
        let Some(running) = found else { return Err(format!("Job {job} isn't running")) };
        let mut control = lock(&running.control);
        let Some((_, folder)) = control.asking.filter(|(asking, _)| *asking == clash) else {
            return Err(format!("Job {job} isn't asking about clash {clash}"));
        };
        match (answer, folder) {
            (ClashAnswer::Replace, true) => return Err("A folder can be merged or skipped".into()),
            (ClashAnswer::Merge, false) => return Err("Only folders can be merged".into()),
            _ => {}
        }
        control.answer = Some((answer, all));
        running.changed.notify_all();
        Ok(())
    }

    /// Stops every job of `client`, which hung up.
    pub fn client_gone(&self, client: ClientId) {
        self.forget(|(owner, _)| *owner == client);
    }

    /// Stops every job, for good.
    pub fn stop(&self) {
        for (_, job) in lock(&self.jobs).take().into_iter().flatten() {
            job.stop();
        }
    }

    /// Unregisters and stops the jobs `which` picks; they send nothing more.
    fn forget(&self, which: impl Fn(&Key) -> bool) {
        let mut jobs = lock(&self.jobs);
        let Some(running) = jobs.as_mut() else { return };
        running.retain(|key, job| {
            let keep = !which(key);
            if !keep {
                job.stop();
            }
            keep
        });
    }
}

impl Job {
    fn stop(&self) {
        lock(&self.control).stopped = true;
        self.changed.notify_all();
    }

    fn stopped(&self) -> bool {
        lock(&self.control).stopped
    }
}

/// Why a job ended early.
enum Halt {
    Stopped,
    Failed(String),
}

type Step = Result<(), Halt>;

/// An item to place, checked.
struct Item {
    source: PathBuf,
    name: OsString,
    meta: Metadata,
    /// Its bytes, as measured.
    bytes: u64,
    /// It's copied into the folder it's in, under a new name.
    renamed: bool,
}

/// The thread that runs one job.
struct Run {
    job: Arc<Job>,
    hub: Arc<Hub>,
    logger: Arc<Logger>,
    jobs: Jobs,
    sources: Vec<PathBuf>,
    folder: PathBuf,
    done_bytes: u64,
    /// The bytes to copy and the clashes still ahead, kept as items are
    /// placed or skipped.
    total: Measure,
    /// Answers given to all file clashes ([0]) or all folder clashes ([1]).
    remembered: [Option<ClashAnswer>; 2],
    next_clash: u32,
    last_report: Instant,
    /// Set while a folder moved to another file system is copied: its
    /// entries stay in place until all of it is, so a stop halfway (which
    /// may put back what it replaced) loses none of them.
    copying_moved: bool,
}

impl Run {
    fn run(mut self) {
        self.report(TransferState::Counting, None, None);
        let result = self.prepare().and_then(|items| {
            self.report(TransferState::Running, None, None);
            items.into_iter().try_for_each(|item| self.place_item(item))
        });
        match result {
            Ok(()) => self.report(TransferState::Done, None, None),
            Err(Halt::Stopped) => self.report(TransferState::Cancelled, None, None),
            Err(Halt::Failed(error)) => {
                self.logger.warn(LOG_SOURCE, &error);
                self.report(TransferState::Failed, None, Some(error));
            }
        }
    }

    /// Checks the job's items and measures them.
    fn prepare(&mut self) -> Result<Vec<Item>, Halt> {
        let folder = fs::canonicalize(&self.folder).map_err(|e| failed(&self.folder, &e))?;
        if !folder.is_dir() {
            return Err(Halt::Failed(format!("{} isn't a folder", self.folder.display())));
        }
        let mut items = Vec::with_capacity(self.sources.len());
        for source in &self.sources {
            let meta = fs::symlink_metadata(source).map_err(|e| failed(source, &e))?;
            let (Some(parent), Some(name)) = (source.parent(), source.file_name()) else {
                return Err(Halt::Failed(format!("Can't {} /", self.verb())));
            };
            let in_place = fs::canonicalize(parent).map_err(|e| failed(parent, &e))? == folder;
            if meta.is_dir() && folder.starts_with(fs::canonicalize(source).map_err(|e| failed(source, &e))?) {
                return Err(Halt::Failed(format!("Can't {} {} into itself", self.verb(), name.to_string_lossy())));
            }
            // Moving an item where it already is leaves it be.
            if in_place && self.job.mode == TransferMode::Move {
                continue;
            }
            let target = (!in_place).then(|| self.folder.join(name));
            let job = Arc::clone(&self.job);
            let measured = copying::measure(source, target.as_deref(), &|| job.stopped())
                .map_err(|e| failed(source, &e))?
                .ok_or(Halt::Stopped)?;
            self.total.add(measured);
            let bytes = measured.bytes;
            items.push(Item { source: source.clone(), name: name.to_owned(), meta, bytes, renamed: in_place });
        }
        Ok(items)
    }

    fn place_item(&mut self, item: Item) -> Step {
        if !item.renamed {
            let folder = self.folder.clone();
            return self.place(&item.source, &folder, &item.name, Some(item.bytes));
        }
        self.check()?;
        let name =
            copying::numbered(&self.folder, &item.name, item.meta.is_dir()).map_err(|e| failed(&item.source, &e))?;
        self.put(&item.source, &item.meta, &self.folder.join(name), None, Some(item.bytes))
    }

    /// Copies or moves `source` into the folder `into` as `name`, asking
    /// when the name is taken. `bytes` is its size when it's known.
    fn place(&mut self, source: &Path, into: &Path, name: &std::ffi::OsStr, bytes: Option<u64>) -> Step {
        self.check()?;
        let meta = fs::symlink_metadata(source).map_err(|e| failed(source, &e))?;
        let target = into.join(name);
        let existing = match fs::symlink_metadata(&target) {
            Ok(existing) => existing,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return self.put(source, &meta, &target, None, bytes),
            Err(e) => return Err(failed(&target, &e)),
        };
        let clashing = Clashing::of(&meta, &existing);
        match self.decide(source, &meta, &target, &existing, clashing)? {
            ClashAnswer::Skip => self.skip(source, &target, clashing),
            ClashAnswer::Merge => self.merge(source, &target),
            ClashAnswer::Replace => {
                if clashing == Clashing::Files {
                    self.total.file_clashes = self.total.file_clashes.saturating_sub(1);
                }
                self.put(source, &meta, &target, Some(&existing), bytes)
            }
        }
    }

    /// What to do about `target` being taken: the answer given to all
    /// clashes of its kind, or the client's.
    fn decide(
        &mut self,
        source: &Path,
        meta: &Metadata,
        target: &Path,
        existing: &Metadata,
        clashing: Clashing,
    ) -> Result<ClashAnswer, Halt> {
        // A folder against a file is never answered for all.
        let kind = match clashing {
            Clashing::Files => Some(0),
            Clashing::Folders => Some(1),
            Clashing::Mixed => None,
        };
        if let Some(answer) = kind.and_then(|kind| self.remembered[kind]) {
            return Ok(answer);
        }
        self.next_clash += 1;
        let id = self.next_clash;
        let folders = clashing == Clashing::Folders;
        let same_kind = match clashing {
            Clashing::Files => self.total.file_clashes,
            Clashing::Folders => self.total.folder_clashes,
            Clashing::Mixed => 0,
        };
        let clash = Clash {
            id,
            name: nova_protocol::wire_text(target.file_name().unwrap_or_default()),
            folder: folders,
            original: copying::clash_item(target, existing),
            replacement: copying::clash_item(source, meta),
            others: same_kind.saturating_sub(1),
        };
        {
            let mut control = lock(&self.job.control);
            if control.stopped {
                return Err(Halt::Stopped);
            }
            control.asking = Some((id, folders));
            control.answer = None;
        }
        self.report(TransferState::Asking, Some(clash), None);
        let mut control = lock(&self.job.control);
        let (answer, all) = loop {
            if control.stopped {
                return Err(Halt::Stopped);
            }
            if let Some(answered) = control.answer.take() {
                control.asking = None;
                break answered;
            }
            control = self.job.changed.wait(control).unwrap_or_else(std::sync::PoisonError::into_inner);
        };
        drop(control);
        if all && let Some(kind) = kind {
            self.remembered[kind] = Some(answer);
        }
        self.report(TransferState::Running, None, None);
        Ok(answer)
    }

    /// Leaves `source` where it is: its bytes and the clashes inside it
    /// come off what's ahead.
    fn skip(&mut self, source: &Path, target: &Path, clashing: Clashing) -> Step {
        let folders = clashing == Clashing::Folders;
        let job = Arc::clone(&self.job);
        let skipped = copying::measure(source, folders.then_some(target), &|| job.stopped())
            .map_err(|e| failed(source, &e))?
            .ok_or(Halt::Stopped)?;
        self.total.bytes = self.total.bytes.saturating_sub(skipped.bytes);
        // Measured with what it clashed with, a skipped folder counts the
        // clashes a merge would have met; anything else counts as itself.
        let (files, folders) = match clashing {
            Clashing::Folders => (skipped.file_clashes, skipped.folder_clashes),
            Clashing::Files => (1, 0),
            Clashing::Mixed => (0, 0),
        };
        self.total.file_clashes = self.total.file_clashes.saturating_sub(files);
        self.total.folder_clashes = self.total.folder_clashes.saturating_sub(folders);
        Ok(())
    }

    /// Places the folder `source`'s entries in the folder `target`; a moved
    /// folder goes once everything in it has.
    fn merge(&mut self, source: &Path, target: &Path) -> Step {
        self.total.folder_clashes = self.total.folder_clashes.saturating_sub(1);
        self.place_children(source, target)?;
        if self.job.mode == TransferMode::Move {
            // Whatever was skipped stays in it.
            let _ = fs::remove_dir(source);
        }
        Ok(())
    }

    fn place_children(&mut self, source: &Path, target: &Path) -> Step {
        let entries = fs::read_dir(source).map_err(|e| failed(source, &e))?;
        for entry in entries {
            let name = entry.map_err(|e| failed(source, &e))?.file_name();
            self.place(&source.join(&name), target, &name, None)?;
        }
        Ok(())
    }

    /// Puts `source` at `target`, replacing `existing` there. A file over
    /// a file is swapped in one step; a folder for a file, or a file for a
    /// folder, sets the old one aside until the new one is in, and puts it
    /// back if that fails or is cancelled. `bytes` is its size when known.
    fn put(
        &mut self,
        source: &Path,
        meta: &Metadata,
        target: &Path,
        existing: Option<&Metadata>,
        bytes: Option<u64>,
    ) -> Step {
        self.check()?;
        let aside = match existing {
            Some(existing) if existing.is_dir() || meta.is_dir() => {
                Some(Aside::new(target).map_err(|e| failed(target, &e))?)
            }
            _ => None,
        };
        let placed = self.put_free(source, meta, target, bytes);
        let Some(aside) = aside else { return placed };
        let settled = if placed.is_ok() { aside.discard() } else { aside.restore() };
        if let Err(e) = settled {
            self.logger.warn(LOG_SOURCE, &format!("can't tidy up after replacing {}: {e}", target.display()));
        }
        placed
    }

    /// Puts `source` at `target`, where only a file can be.
    fn put_free(&mut self, source: &Path, meta: &Metadata, target: &Path, bytes: Option<u64>) -> Step {
        let moving = self.job.mode == TransferMode::Move && !self.copying_moved;
        if moving {
            match fs::rename(source, target) {
                Ok(()) => {
                    let bytes = bytes.unwrap_or_else(|| moved_bytes(target, meta));
                    self.copied(bytes);
                    return Ok(());
                }
                // Another file system: copied, then deleted.
                Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {}
                Err(e) => return Err(failed(source, &e)),
            }
        }
        if moving && meta.is_dir() {
            self.copying_moved = true;
            let placed = self.copy(source, meta, target);
            self.copying_moved = false;
            // Once all of it is copied the move has happened: what can't
            // be deleted after that stays at the source too, and nothing
            // placed is undone.
            if placed? && let Err(e) = copying::remove_moved(source) {
                self.logger.warn(LOG_SOURCE, &format!("moved {}, but can't delete all of it: {e}", source.display()));
            }
            return Ok(());
        }
        let placed = self.copy(source, meta, target)?;
        if placed && moving {
            fs::remove_file(source).map_err(|e| failed(source, &e))?;
        }
        Ok(())
    }

    /// Copies `source` to the free `target`. False for what can't be
    /// copied (a device, a socket, a pipe), which is left out.
    fn copy(&mut self, source: &Path, meta: &Metadata, target: &Path) -> Result<bool, Halt> {
        let file_type = meta.file_type();
        if file_type.is_file() {
            let into = target.parent().unwrap_or(Path::new("/"));
            let mut part = Part::create(into).map_err(|e| failed(target, &e))?;
            let filled = part.fill(source, &mut |bytes| {
                self.copied(bytes);
                !self.job.stopped()
            });
            if !filled.map_err(|e| failed(source, &e))? {
                return Err(Halt::Stopped);
            }
            part.finish(meta, target).map_err(|e| failed(target, &e))?;
        } else if file_type.is_symlink() {
            copying::copy_link(source, target).map_err(|e| failed(target, &e))?;
        } else if file_type.is_dir() {
            // Private while it fills, so what the source folder kept from
            // other users isn't open to them meanwhile; its own mode after.
            fs::DirBuilder::new().mode(0o700).create(target).map_err(|e| failed(target, &e))?;
            self.place_children(source, target)?;
            // Best effort, after its entries are in: a read-only folder
            // would refuse them.
            let _ = fs::set_permissions(target, meta.permissions());
            if let (Ok(modified), Ok(folder)) = (meta.modified(), fs::File::open(target)) {
                let _ = folder.set_modified(modified);
            }
        } else {
            self.logger.warn(LOG_SOURCE, &format!("left out {}: not a file, folder or link", source.display()));
            return Ok(false);
        }
        Ok(true)
    }

    fn copied(&mut self, bytes: u64) {
        self.done_bytes += bytes;
        if self.last_report.elapsed() >= PROGRESS_EVERY {
            self.report(TransferState::Running, None, None);
        }
    }

    fn check(&self) -> Step {
        if self.job.stopped() { Err(Halt::Stopped) } else { Ok(()) }
    }

    fn verb(&self) -> &'static str {
        match self.job.mode {
            TransferMode::Copy => "copy",
            TransferMode::Move => "move",
        }
    }

    /// Tells the client how the job is going, while it's still registered;
    /// a last state also unregisters it.
    fn report(&mut self, state: TransferState, clash: Option<Clash>, error: Option<String>) {
        self.last_report = Instant::now();
        let last = matches!(state, TransferState::Done | TransferState::Cancelled | TransferState::Failed);
        let (client, job) = self.job.key;
        let transfer = Transfer {
            job,
            mode: self.job.mode,
            state,
            items: self.job.items,
            name: self.job.name.clone(),
            done_bytes: self.done_bytes.min(self.total.bytes),
            total_bytes: self.total.bytes,
            clash,
            error,
        };
        let mut jobs = lock(&self.jobs);
        let Some(running) = jobs.as_mut() else { return };
        if !running.get(&self.job.key).is_some_and(|shown| Arc::ptr_eq(shown, &self.job)) {
            return;
        }
        self.hub.send_to(client, &Event::FilesTransfer(transfer));
        if last {
            running.remove(&self.job.key);
        }
    }
}

/// The bytes of what was just moved to `target` in one rename, for the
/// progress: a file's size, or what a folder holds (0 if it can't be read).
fn moved_bytes(target: &Path, meta: &Metadata) -> u64 {
    if meta.is_dir() {
        copying::measure(target, None, &|| false).ok().flatten().map_or(0, |measured| measured.bytes)
    } else if meta.is_file() {
        meta.len()
    } else {
        0
    }
}

/// Why `path` couldn't be copied or moved, for the window.
fn failed(path: &Path, error: &io::Error) -> Halt {
    let path = path.display();
    Halt::Failed(match error.kind() {
        io::ErrorKind::NotFound => format!("{path} doesn't exist"),
        io::ErrorKind::PermissionDenied => format!("No permission for {path}"),
        io::ErrorKind::StorageFull => format!("No space left for {path}"),
        io::ErrorKind::ReadOnlyFilesystem => format!("{path} is on a read-only drive"),
        _ => format!("Can't copy {path}: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{ClientQueue, Outgoing, next_named};
    use nova_protocol::LogLevel;
    use nova_test_support::TempDir;
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc::Receiver;

    const WAIT: Duration = Duration::from_secs(5);

    /// One client's jobs, between a `from` and a `to` folder.
    struct Fixture {
        dir: TempDir,
        transfers: Transfers,
        client: ClientId,
        events: Receiver<Outgoing>,
        _socket: UnixStream,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir = TempDir::new(&format!("transfers-{name}"));
            fs::create_dir_all(dir.join("from")).unwrap();
            fs::create_dir_all(dir.join("to")).unwrap();
            let hub = Arc::new(Hub::default());
            let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, Arc::clone(&hub)).unwrap());
            let (socket, ours) = UnixStream::pair().unwrap();
            let (queue, events) = ClientQueue::new(&ours).unwrap();
            let client = hub.register(queue);
            Self { transfers: Transfers::new(hub, logger), dir, client, events, _socket: socket }
        }

        fn write(&self, path: &str, content: &str) {
            let path = self.dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }

        fn read(&self, path: &str) -> String {
            fs::read_to_string(self.dir.join(path)).unwrap()
        }

        fn exists(&self, path: &str) -> bool {
            fs::symlink_metadata(self.dir.join(path)).is_ok()
        }

        fn start(&self, mode: TransferMode, sources: &[&str], folder: &str) {
            let sources: Vec<_> = sources.iter().map(|source| self.dir.join(source).display().to_string()).collect();
            let folder = self.dir.join(folder).display().to_string();
            self.transfers.start(self.client, 1, mode, &sources, &folder).unwrap();
        }

        /// The job's events up to the first in `state`, which is returned.
        fn until(&self, state: TransferState) -> Transfer {
            loop {
                let transfer: Transfer =
                    serde_json::from_value(next_named(&self.events, "files_transfer", WAIT)).unwrap();
                if transfer.state == state {
                    return transfer;
                }
                assert!(
                    !matches!(transfer.state, TransferState::Done | TransferState::Cancelled | TransferState::Failed),
                    "ended {:?} ({:?}) waiting for {state:?}",
                    transfer.state,
                    transfer.error
                );
            }
        }

        fn answer(&self, clash: &Clash, answer: ClashAnswer, all: bool) {
            self.transfers.answer(self.client, 1, clash.id, answer, all).unwrap();
        }

        fn names(&self, folder: &str) -> Vec<String> {
            let mut names: Vec<_> = fs::read_dir(self.dir.join(folder))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    #[test]
    fn a_copy_brings_files_folders_and_links_with_their_times() {
        let fixture = Fixture::new("copy");
        fixture.write("from/Photos/2025/a.png", "12345");
        fixture.write("from/notes.txt", "hi");
        std::os::unix::fs::symlink("2025/a.png", fixture.dir.join("from/Photos/latest")).unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        fs::File::options().write(true).open(fixture.dir.join("from/notes.txt")).unwrap().set_modified(old).unwrap();
        fixture.start(TransferMode::Copy, &["from/Photos", "from/notes.txt"], "to");
        let counting = fixture.until(TransferState::Counting);
        assert_eq!((counting.items, counting.name.as_str()), (2, "Photos"));
        let done = fixture.until(TransferState::Done);
        assert_eq!((done.done_bytes, done.total_bytes), (7, 7));
        assert_eq!(fixture.read("to/Photos/2025/a.png"), "12345");
        assert_eq!(fs::read_link(fixture.dir.join("to/Photos/latest")).unwrap(), Path::new("2025/a.png"));
        assert_eq!(fs::metadata(fixture.dir.join("to/notes.txt")).unwrap().modified().unwrap(), old);
        assert!(fixture.exists("from/notes.txt"), "a copy leaves the original");
        assert_eq!(fixture.names("to/Photos"), ["2025", "latest"], "no partial files left");
    }

    #[test]
    fn a_copy_into_its_own_folder_gets_a_number_and_a_move_there_stays() {
        let fixture = Fixture::new("same-folder");
        fixture.write("from/cover.png", "png");
        fixture.start(TransferMode::Copy, &["from/cover.png"], "from");
        fixture.until(TransferState::Done);
        assert_eq!(fixture.names("from"), ["cover (2).png", "cover.png"]);
        fixture.start(TransferMode::Move, &["from/cover.png"], "from");
        fixture.until(TransferState::Done);
        assert_eq!(fixture.names("from"), ["cover (2).png", "cover.png"]);
    }

    #[test]
    fn a_taken_file_name_asks_and_replace_for_all_answers_the_rest() {
        let fixture = Fixture::new("replace");
        for name in ["a.txt", "b.txt", "c.txt"] {
            fixture.write(&format!("from/{name}"), "new");
            fixture.write(&format!("to/{name}"), "old!");
        }
        fixture.start(TransferMode::Copy, &["from/a.txt", "from/b.txt", "from/c.txt"], "to");
        let asking = fixture.until(TransferState::Asking);
        let clash = asking.clash.unwrap();
        assert_eq!((clash.name.as_str(), clash.folder, clash.others), ("a.txt", false, 2));
        assert_eq!((clash.original.size, clash.replacement.size), (Some(4), Some(3)));
        fixture.answer(&clash, ClashAnswer::Skip, false);
        let clash = fixture.until(TransferState::Asking).clash.unwrap();
        assert_eq!((clash.name.as_str(), clash.others), ("b.txt", 1));
        assert!(
            fixture.transfers.answer(fixture.client, 1, clash.id, ClashAnswer::Merge, false).is_err(),
            "files don't merge"
        );
        fixture.answer(&clash, ClashAnswer::Replace, true);
        let done = fixture.until(TransferState::Done);
        assert_eq!(done.total_bytes, 6, "the skipped file comes off the total");
        assert_eq!(
            [fixture.read("to/a.txt"), fixture.read("to/b.txt"), fixture.read("to/c.txt")],
            ["old!", "new", "new"]
        );
    }

    #[test]
    fn a_file_on_a_folder_is_always_asked_about_and_replaces_it_whole() {
        let fixture = Fixture::new("mixed");
        fixture.write("from/a.txt", "new");
        fixture.write("to/a.txt", "old");
        fixture.write("from/Photos", "a file now");
        fixture.write("to/Photos/kept.png", "png");
        fixture.start(TransferMode::Copy, &["from/a.txt", "from/Photos"], "to");
        let clash = fixture.until(TransferState::Asking).clash.unwrap();
        assert_eq!(clash.others, 0, "a file on a folder isn't one of the file clashes");
        fixture.answer(&clash, ClashAnswer::Replace, true);
        let mixed = fixture.until(TransferState::Asking).clash.unwrap();
        assert_eq!((mixed.name.as_str(), mixed.folder, mixed.original.items), ("Photos", false, Some(1)));
        fixture.answer(&mixed, ClashAnswer::Replace, false);
        fixture.until(TransferState::Done);
        assert_eq!(fixture.read("to/Photos"), "a file now");
        assert_eq!(fixture.names("to"), ["Photos", "a.txt"], "nothing set aside is left");
    }

    #[test]
    fn a_folder_on_a_folder_merges_and_asks_about_the_files_inside() {
        let fixture = Fixture::new("merge");
        fixture.write("from/Photos/new.png", "new");
        fixture.write("from/Photos/same.png", "theirs");
        fixture.write("to/Photos/old.png", "old");
        fixture.write("to/Photos/same.png", "mine");
        fixture.start(TransferMode::Move, &["from/Photos"], "to");
        let clash = fixture.until(TransferState::Asking).clash.unwrap();
        assert_eq!((clash.name.as_str(), clash.folder), ("Photos", true));
        assert_eq!((clash.original.items, clash.replacement.items), (Some(2), Some(2)));
        assert!(fixture.transfers.answer(fixture.client, 1, clash.id, ClashAnswer::Replace, false).is_err());
        fixture.answer(&clash, ClashAnswer::Merge, false);
        let inner = fixture.until(TransferState::Asking).clash.unwrap();
        assert_eq!((inner.name.as_str(), inner.folder), ("same.png", false));
        fixture.answer(&inner, ClashAnswer::Skip, false);
        fixture.until(TransferState::Done);
        assert_eq!(fixture.names("to/Photos"), ["new.png", "old.png", "same.png"]);
        assert_eq!(fixture.read("to/Photos/same.png"), "mine");
        assert_eq!(fixture.names("from/Photos"), ["same.png"], "what was skipped stays where it was");
    }

    #[test]
    fn cancelling_while_asking_ends_the_job_and_keeps_what_was_there() {
        let fixture = Fixture::new("cancel");
        fixture.write("from/a.txt", "new");
        fixture.write("to/a.txt", "old");
        fixture.start(TransferMode::Copy, &["from/a.txt"], "to");
        fixture.until(TransferState::Asking);
        fixture.transfers.cancel(fixture.client, 1);
        fixture.until(TransferState::Cancelled);
        assert_eq!(fixture.read("to/a.txt"), "old");
        assert_eq!(fixture.names("to"), ["a.txt"]);
    }

    #[test]
    fn a_folder_cant_go_into_itself() {
        let fixture = Fixture::new("itself");
        fixture.write("from/Photos/a.png", "png");
        fixture.start(TransferMode::Copy, &["from/Photos"], "from/Photos");
        let failed = fixture.until(TransferState::Failed);
        assert!(failed.error.unwrap().contains("into itself"));
        assert_eq!(fixture.names("from/Photos"), ["a.png"]);
    }

    #[test]
    fn a_job_number_runs_once_and_requests_are_checked() {
        let fixture = Fixture::new("checks");
        fixture.write("from/a.txt", "a");
        fixture.write("to/a.txt", "b");
        fixture.start(TransferMode::Copy, &["from/a.txt"], "to");
        fixture.until(TransferState::Asking);
        let again = fixture.transfers.start(fixture.client, 1, TransferMode::Copy, &["/tmp".into()], "/tmp");
        assert!(again.unwrap_err().contains("already running"));
        assert!(fixture.transfers.start(fixture.client, 2, TransferMode::Copy, &[], "/tmp").is_err());
        assert!(fixture.transfers.start(fixture.client, 2, TransferMode::Copy, &["relative".into()], "/tmp").is_err());
        fixture.transfers.client_gone(fixture.client);
        assert!(
            fixture.transfers.answer(fixture.client, 1, 1, ClashAnswer::Skip, false).is_err(),
            "gone with its client"
        );
    }
}
