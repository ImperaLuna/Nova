//! A thin wrapper over Linux inotify: novad learns about changed files (new
//! Codex log lines, edited settings) when they're written instead of checking
//! on a timer.

use std::collections::HashMap;
use std::ffi::{CString, OsStr};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Appends, new files and folders, and files moved in.
pub const WRITES: u32 = libc::IN_MODIFY | libc::IN_CREATE | libc::IN_MOVED_TO;
/// `WRITES`, plus files deleted or moved away: an editor that saves by
/// renaming moves the old file out first.
pub const REPLACEMENTS: u32 = WRITES | libc::IN_DELETE | libc::IN_MOVED_FROM;
/// Entries that come or go in a folder: created, moved in, replaced by a
/// rename (`ln -sfn` repointing a symlink), deleted or moved away. For a
/// folder whose files don't matter, only one entry in it, so writes to its
/// other files cost nothing.
pub const ENTRIES: u32 = libc::IN_CREATE | libc::IN_MOVED_TO | libc::IN_DELETE | libc::IN_MOVED_FROM;
/// `struct inotify_event` before its name: wd, mask, cookie, len.
const HEADER_BYTES: usize = 16;
const BUFFER_BYTES: usize = 64 * 1024;

/// Something changed inside a watched folder.
pub struct Change {
    pub path: PathBuf,
    pub is_dir: bool,
}

/// What one `Inotify::read` saw.
pub struct Changes {
    pub found: Vec<Change>,
    /// The kernel's queue filled up and dropped events, so `found` may miss
    /// changes: look again at everything watched.
    pub overflowed: bool,
}

pub struct Inotify {
    file: std::fs::File,
    mask: u32,
    dirs: HashMap<i32, PathBuf>,
    stop: Stopper,
}

/// Ends a blocked or future `Inotify::read`, from any thread.
#[derive(Clone)]
pub struct Stopper(Arc<std::fs::File>);

impl Stopper {
    pub fn stop(&self) {
        // An eventfd adds the 8-byte number written to its counter.
        if let Err(e) = (&*self.0).write_all(&1u64.to_ne_bytes()) {
            eprintln!("novad: can't stop a file watcher: {e}");
        }
    }
}

impl Inotify {
    /// Watches for the events in `mask` (`WRITES`, `REPLACEMENTS`).
    pub fn new(mask: u32) -> std::io::Result<Self> {
        // SAFETY: no pointers; returns a new descriptor or -1.
        let file = owned(unsafe { libc::inotify_init1(libc::IN_CLOEXEC) })?;
        // SAFETY: as above.
        let stop = owned(unsafe { libc::eventfd(0, libc::EFD_CLOEXEC) })?;
        Ok(Self { file: file.into(), mask, dirs: HashMap::new(), stop: Stopper(Arc::new(stop.into())) })
    }

    /// A handle that makes `read` return `None`.
    pub fn stopper(&self) -> Stopper {
        self.stop.clone()
    }

    /// Watches `dir` and every folder below it.
    pub fn watch_tree(&mut self, dir: &Path) -> std::io::Result<()> {
        self.watch(dir)?;
        for entry in std::fs::read_dir(dir)?.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                self.watch_tree(&entry.path())?;
            }
        }
        Ok(())
    }

    /// Watches the files directly in `dir`.
    pub fn watch(&mut self, dir: &Path) -> std::io::Result<()> {
        self.watch_for(dir, self.mask)
    }

    /// Watches `dir` for the events in `mask` only, like [`ENTRIES`].
    pub fn watch_for(&mut self, dir: &Path, mask: u32) -> std::io::Result<()> {
        let path = CString::new(dir.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
        // SAFETY: `path` is NUL-terminated and outlives the call.
        let wd = unsafe { libc::inotify_add_watch(self.file.as_raw_fd(), path.as_ptr(), mask) };
        if wd == -1 {
            return Err(std::io::Error::last_os_error());
        }
        self.dirs.insert(wd, dir.to_owned());
        Ok(())
    }

    /// Blocks until something changes, or `None` once stopped.
    pub fn read(&mut self) -> std::io::Result<Option<Changes>> {
        if !self.wait()? {
            return Ok(None);
        }
        let mut buffer = vec![0u8; BUFFER_BYTES];
        let read = self.file.read(&mut buffer)?;
        let mut changes = Changes { found: Vec::new(), overflowed: false };
        let mut at = 0;
        while at + HEADER_BYTES <= read {
            let field = |offset: usize| {
                let bytes = buffer[at + offset..at + offset + 4].try_into().unwrap_or([0; 4]);
                u32::from_ne_bytes(bytes)
            };
            let wd = i32::from_ne_bytes(field(0).to_ne_bytes());
            let mask = field(4);
            let name_len = usize::try_from(field(12)).unwrap_or(0);
            let name = &buffer[at + HEADER_BYTES..(at + HEADER_BYTES + name_len).min(read)];
            at += HEADER_BYTES + name_len;
            if mask & libc::IN_Q_OVERFLOW != 0 {
                changes.overflowed = true;
                continue;
            }
            if mask & libc::IN_IGNORED != 0 {
                self.dirs.remove(&wd);
                continue;
            }
            let name = name.split(|byte| *byte == 0).next().unwrap_or_default();
            if let (Some(dir), false) = (self.dirs.get(&wd), name.is_empty()) {
                let path = dir.join(OsStr::from_bytes(name));
                changes.found.push(Change { path, is_dir: mask & libc::IN_ISDIR != 0 });
            }
        }
        Ok(Some(changes))
    }

    /// Waits for changes (true) or a stop (false).
    fn wait(&self) -> std::io::Result<bool> {
        let mut fds = [self.file.as_raw_fd(), self.stop.0.as_raw_fd()].map(|fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        });
        loop {
            // SAFETY: `fds` is a valid array of `fds.len()` pollfds for the call.
            if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) } != -1 {
                return Ok(fds[1].revents == 0);
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
}

fn owned(fd: libc::c_int) -> std::io::Result<OwnedFd> {
    if fd == -1 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `fd` was just opened and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// More events than the kernel queues are written while nobody reads.
    #[test]
    fn a_full_queue_is_reported_as_an_overflow() {
        let dir = std::env::temp_dir().join(format!("nova-inotify-overflow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut watcher = Inotify::new(WRITES).unwrap();
        watcher.watch(&dir).unwrap();
        let limit: usize =
            std::fs::read_to_string("/proc/sys/fs/inotify/max_queued_events").unwrap().trim().parse().unwrap();
        // Two files in turn: the kernel merges a repeat of the event just before.
        let files = [dir.join("a"), dir.join("b")].map(|path| std::fs::File::create(path).unwrap());
        for i in 0..=limit {
            (&files[i % 2]).write_all(b"x").unwrap();
        }
        // Without an overflow, `read` would wait forever once the queue is empty.
        let stopper = watcher.stopper();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            stopper.stop();
        });
        let overflowed = std::iter::from_fn(|| watcher.read().unwrap()).any(|changes| changes.overflowed);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(overflowed, "the dropped events went unnoticed");
    }
}
