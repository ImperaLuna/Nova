//! A thin wrapper over Linux inotify: novad learns about changed files (new
//! Codex log lines, edited settings, a folder open in Nova Files) when
//! they're written instead of checking on a timer.
//!
//! One watcher can be shared between threads: one blocks in `read` while
//! others add and drop watches. Two paths that name one folder (a link and
//! its target) share the kernel's watch, and its events are reported under
//! both.

use std::collections::HashMap;
use std::ffi::{CString, OsStr};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::lock;

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
/// A watched folder itself deleted, moved or unmounted (reported in
/// [`Changes::gone`]).
pub const ITSELF: u32 = libc::IN_DELETE_SELF | libc::IN_MOVE_SELF | libc::IN_UNMOUNT;
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
    /// Watched folders that were deleted, moved or unmounted, for a watch
    /// with [`ITSELF`].
    pub gone: Vec<PathBuf>,
}

impl Changes {
    /// Nothing seen.
    pub fn none() -> Self {
        Self { found: Vec::new(), overflowed: false, gone: Vec::new() }
    }
}

pub struct Inotify {
    file: std::fs::File,
    mask: u32,
    /// The paths each watch was added for.
    dirs: Mutex<HashMap<i32, Vec<PathBuf>>>,
    stop: Stopper,
}

/// Ends a blocked or future `Inotify::read` (or any wait that polls its
/// eventfd, like a hidraw reader's), from any thread. Once stopped, it
/// stays stopped: the eventfd is never read.
#[derive(Clone)]
pub struct Stopper(Arc<std::fs::File>);

impl Stopper {
    pub fn new() -> std::io::Result<Self> {
        // SAFETY: no pointers; returns a new descriptor or -1.
        let stop = owned(unsafe { libc::eventfd(0, libc::EFD_CLOEXEC) })?;
        Ok(Self(Arc::new(stop.into())))
    }

    pub fn stop(&self) {
        // An eventfd adds the 8-byte number written to its counter.
        if let Err(e) = (&*self.0).write_all(&1u64.to_ne_bytes()) {
            eprintln!("novad: can't stop a watcher: {e}");
        }
    }

    /// Whether it was stopped, without waiting.
    pub fn stopped(&self) -> bool {
        let mut fd = libc::pollfd { fd: self.0.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: `fd` is one valid pollfd for the call.
        unsafe { libc::poll(&raw mut fd, 1, 0) > 0 }
    }
}

impl AsRawFd for Stopper {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.0.as_raw_fd()
    }
}

impl Inotify {
    /// Watches for the events in `mask` (`WRITES`, `REPLACEMENTS`).
    pub fn new(mask: u32) -> std::io::Result<Self> {
        // SAFETY: no pointers; returns a new descriptor or -1.
        let file = owned(unsafe { libc::inotify_init1(libc::IN_CLOEXEC) })?;
        Ok(Self { file: file.into(), mask, dirs: Mutex::default(), stop: Stopper::new()? })
    }

    /// A handle that makes `read` return `None`.
    pub fn stopper(&self) -> Stopper {
        self.stop.clone()
    }

    /// Watches `dir` and every folder below it.
    pub fn watch_tree(&self, dir: &Path) -> std::io::Result<()> {
        self.watch(dir)?;
        for entry in std::fs::read_dir(dir)?.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                self.watch_tree(&entry.path())?;
            }
        }
        Ok(())
    }

    /// Watches the files directly in `dir`.
    pub fn watch(&self, dir: &Path) -> std::io::Result<()> {
        self.watch_for(dir, self.mask)
    }

    /// Watches `dir` for the events in `mask` only, like [`ENTRIES`]. Watched
    /// again, `dir` is followed to what it names now: an older watch of it
    /// that no other path shares is dropped. Events that come while the
    /// watch is being added may be missed, so read `dir` after watching it.
    pub fn watch_for(&self, dir: &Path, mask: u32) -> std::io::Result<()> {
        let path = CString::new(dir.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
        // SAFETY: `path` is NUL-terminated and outlives the call.
        let wd = unsafe { libc::inotify_add_watch(self.file.as_raw_fd(), path.as_ptr(), mask) };
        if wd == -1 {
            return Err(std::io::Error::last_os_error());
        }
        let mut dirs = lock(&self.dirs);
        self.forget(&mut dirs, dir, Some(wd));
        let paths = dirs.entry(wd).or_default();
        if !paths.iter().any(|path| path == dir) {
            paths.push(dir.to_owned());
        }
        Ok(())
    }

    /// Stops watching `dir`, whatever it pointed to when it was watched.
    pub fn unwatch(&self, dir: &Path) {
        self.forget(&mut lock(&self.dirs), dir, None);
    }

    /// Takes `dir` off every watch but `keep`, dropping the watches no path
    /// is left on.
    fn forget(&self, dirs: &mut HashMap<i32, Vec<PathBuf>>, dir: &Path, keep: Option<i32>) {
        dirs.retain(|wd, paths| {
            if Some(*wd) == keep {
                return true;
            }
            paths.retain(|path| path != dir);
            if paths.is_empty() {
                // SAFETY: no pointers; a stale `wd` only fails with EINVAL.
                unsafe { libc::inotify_rm_watch(self.file.as_raw_fd(), *wd) };
            }
            !paths.is_empty()
        });
    }

    /// Stops watching the folders `keep` says no to, like ones a swapped
    /// link no longer leads to.
    pub fn unwatch_unless(&self, keep: impl Fn(&Path) -> bool) {
        lock(&self.dirs).retain(|wd, paths| {
            paths.retain(|path| keep(path));
            if paths.is_empty() {
                // SAFETY: no pointers; a stale `wd` only fails with EINVAL.
                unsafe { libc::inotify_rm_watch(self.file.as_raw_fd(), *wd) };
            }
            !paths.is_empty()
        });
    }

    /// Blocks until something changes, or `None` once stopped.
    pub fn read(&self) -> std::io::Result<Option<Changes>> {
        self.read_until(None)
    }

    /// Like [`Inotify::read`], but waits only until `deadline`: then the
    /// changes are empty.
    pub fn read_until(&self, deadline: Option<Instant>) -> std::io::Result<Option<Changes>> {
        match self.wait(deadline)? {
            Woke::Stopped => return Ok(None),
            Woke::TimedOut => return Ok(Some(Changes::none())),
            Woke::Changed => {}
        }
        let mut buffer = vec![0u8; BUFFER_BYTES];
        let read = (&self.file).read(&mut buffer)?;
        let mut dirs = lock(&self.dirs);
        let mut changes = Changes::none();
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
                dirs.remove(&wd);
                continue;
            }
            let paths = dirs.get(&wd).map(Vec::as_slice).unwrap_or_default();
            if mask & ITSELF != 0 {
                changes.gone.extend(paths.iter().cloned());
                continue;
            }
            let name = name.split(|byte| *byte == 0).next().unwrap_or_default();
            if !name.is_empty() {
                let is_dir = mask & libc::IN_ISDIR != 0;
                let found = paths.iter().map(|dir| Change { path: dir.join(OsStr::from_bytes(name)), is_dir });
                changes.found.extend(found);
            }
        }
        Ok(Some(changes))
    }

    /// Waits for changes, a stop, or `deadline`.
    fn wait(&self, deadline: Option<Instant>) -> std::io::Result<Woke> {
        let mut fds = [self.file.as_raw_fd(), self.stop.0.as_raw_fd()].map(|fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        });
        loop {
            // Rounded up, so a wait never ends just before its deadline.
            let timeout = deadline.map_or(-1, |deadline| {
                let left = deadline.saturating_duration_since(Instant::now());
                libc::c_int::try_from(left.as_micros().div_ceil(1000)).unwrap_or(libc::c_int::MAX)
            });
            // SAFETY: `fds` is a valid array of `fds.len()` pollfds for the call.
            let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
            if ready == 0 {
                return Ok(Woke::TimedOut);
            }
            if ready != -1 {
                return Ok(if fds[1].revents == 0 { Woke::Changed } else { Woke::Stopped });
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
}

/// Why [`Inotify::wait`] returned.
enum Woke {
    Changed,
    Stopped,
    TimedOut,
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
        let dir = nova_test_support::TempDir::new("inotify-overflow");
        let watcher = Inotify::new(WRITES).unwrap();
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
        assert!(overflowed, "the dropped events went unnoticed");
    }
}
