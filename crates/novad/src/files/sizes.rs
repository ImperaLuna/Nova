//! Folder sizes, for the Size column and "What uses space": everything
//! under a folder added up ([`count`]), about as `du --apparent-size -x`
//! does it. Files count by their length; a file with several hard links
//! counts once; links aren't followed and add nothing; folders that are another drive
//! (a mount inside it) and folders that can't be read are left out.
//!
//! Each folder view counts its subfolders on a thread of its own
//! ([`Counter`]), at the disk's idle priority so counting never slows what
//! people are doing: all of them once the folder is read, and each again
//! when its entry changes. A count stops as soon as the view closes.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

/// `ioprio_set`'s "this thread", its idle class, and where the class sits
/// in the priority value (linux/ioprio.h).
const IOPRIO_WHO_PROCESS: libc::c_long = 1;
const IOPRIO_CLASS_IDLE: libc::c_long = 3;
const IOPRIO_CLASS_SHIFT: libc::c_long = 13;

/// Bytes under `folder` on its own drive, or `None` once `stopped` says
/// the count is no longer wanted (asked before each folder is read). An
/// error only when `folder` itself can't be looked at.
pub fn count(folder: &Path, stopped: impl Fn() -> bool) -> std::io::Result<Option<u64>> {
    let top = std::fs::symlink_metadata(folder)?;
    let drive = top.dev();
    let mut seen_links = HashSet::new();
    let mut total = 0u64;
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        if stopped() {
            return Ok(None);
        }
        // A folder that can't be read (no permission, gone meanwhile)
        // adds nothing.
        let Ok(found) = std::fs::read_dir(&dir) else { continue };
        for entry in found.flatten() {
            let Ok(metadata) = entry.metadata() else { continue };
            if metadata.is_dir() {
                if metadata.dev() == drive {
                    pending.push(entry.path());
                }
            } else if metadata.is_file() && counts_once(&metadata, &mut seen_links) {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(Some(total))
}

/// Whether a file adds to the total: always, unless it's another hard
/// link to one already counted.
fn counts_once(metadata: &Metadata, seen: &mut HashSet<(u64, u64)>) -> bool {
    metadata.nlink() <= 1 || seen.insert((metadata.dev(), metadata.ino()))
}

/// Whether `path` is a folder (not a link to one) on drive `drive`: one a
/// view counts.
fn same_drive_folder(path: &Path, drive: u64) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir() && metadata.dev() == drive)
}

/// What a view's count thread is asked: subfolders to count, by name.
pub struct Counter {
    asks: Sender<OsString>,
}

impl Counter {
    /// Starts counting subfolders of `folder` as they're asked for,
    /// calling `counted(name, bytes)` for each. Ends once the counter is
    /// dropped or `stopped` says so; `None` when no thread could start.
    pub fn start(
        folder: PathBuf,
        stopped: impl Fn() -> bool + Send + 'static,
        counted: impl Fn(&OsString, u64) + Send + 'static,
    ) -> Option<Self> {
        let (asks, asked) = channel();
        let thread = std::thread::Builder::new().name("novad-files-sizes".into());
        thread.spawn(move || run(&folder, &asked, &stopped, &counted)).ok()?;
        Some(Self { asks })
    }

    /// Counts the subfolder `name` (again).
    pub fn ask(&self, name: OsString) {
        // Only fails once the thread has stopped, with its view.
        let _ = self.asks.send(name);
    }
}

fn run(folder: &Path, asked: &Receiver<OsString>, stopped: &dyn Fn() -> bool, counted: &dyn Fn(&OsString, u64)) {
    lower_disk_priority();
    let Ok(drive) = std::fs::metadata(folder).map(|metadata| metadata.dev()) else { return };
    // Asked, not counted yet, in the order asked; one asked twice is
    // counted once.
    let mut queue: Vec<OsString> = Vec::new();
    loop {
        if queue.is_empty() {
            let Ok(name) = asked.recv() else { return };
            queue.push(name);
        }
        queue.extend(asked.try_iter());
        let mut seen = HashSet::new();
        queue.retain(|name| seen.insert(name.clone()));
        let name = queue.remove(0);
        let path = folder.join(&name);
        if !same_drive_folder(&path, drive) {
            continue;
        }
        match count(&path, stopped) {
            Ok(Some(bytes)) => counted(&name, bytes),
            Ok(None) => return,
            // Gone since it was asked for: its entry's change follows.
            Err(_) => {}
        }
    }
}

/// Puts this thread's disk reads in the idle class, where they wait for
/// every other process's. Counting still works when the kernel refuses.
fn lower_disk_priority() {
    let priority = IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT;
    // SAFETY: ioprio_set takes three integers and touches no memory.
    unsafe {
        libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, 0 as libc::c_long, priority);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn write(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn counts_everything_under_a_folder_once() {
        let dir = TempDir::new("files-sizes-count");
        write(&dir.join("a/one"), 100);
        write(&dir.join("a/deep/down/two"), 20);
        write(&dir.join("a/.hidden/three"), 3);
        std::fs::hard_link(dir.join("a/one"), dir.join("a/deep/same")).unwrap();
        // A link isn't followed: what it names is counted where it is.
        write(&dir.join("elsewhere/big"), 5000);
        std::os::unix::fs::symlink(dir.join("elsewhere"), dir.join("a/shortcut")).unwrap();
        std::os::unix::fs::symlink(dir.join("elsewhere/big"), dir.join("a/file-link")).unwrap();
        assert_eq!(count(&dir.join("a"), || false).unwrap(), Some(123));
    }

    #[test]
    fn a_stopped_count_gives_up() {
        let dir = TempDir::new("files-sizes-stop");
        write(&dir.join("a/one"), 1);
        assert_eq!(count(&dir.join("a"), || true).unwrap(), None);
        assert!(count(&dir.join("missing"), || false).is_err());
    }

    #[test]
    fn a_counter_counts_what_it_is_asked() {
        let dir = TempDir::new("files-sizes-counter");
        write(&dir.join("Games/save"), 40);
        write(&dir.join("Music/song"), 7);
        write(&dir.join("file"), 1);
        let (done, results) = channel();
        let done = Arc::new(Mutex::new(done));
        let counter = Counter::start(
            dir.to_path_buf(),
            || false,
            move |name, bytes| drop(done.lock().unwrap().send((name.clone(), bytes))),
        )
        .unwrap();
        for name in ["Games", "file", "missing", "Music"] {
            counter.ask(name.into());
        }
        let wait = Duration::from_secs(5);
        assert_eq!(results.recv_timeout(wait).unwrap(), ("Games".into(), 40));
        assert_eq!(results.recv_timeout(wait).unwrap(), ("Music".into(), 7));
        drop(counter);
        assert!(results.recv_timeout(Duration::from_millis(200)).is_err(), "a file or a missing name isn't counted");
    }
}
