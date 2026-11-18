//! A thin wrapper over Linux inotify: novad learns about changed files (new
//! Codex log lines, edited settings, a new wallpaper) when they're written
//! instead of checking on a timer.
//!
//! One watcher can be shared between threads: one blocks in `read` while
//! others add and drop watches. Two paths that name one folder (a link and
//! its target) share the kernel's watch, and its events are reported under
//! both.

use std::collections::{BTreeMap, HashMap};
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
    dirs: Mutex<Watches>,
    stop: Stopper,
}

/// Which paths are watched, both ways round, so adding or dropping one
/// touches only its own watch however many there are.
#[derive(Default)]
struct Watches {
    /// The paths each watch was added for.
    paths: HashMap<i32, Vec<PathBuf>>,
    /// The watch each path is on.
    wds: BTreeMap<PathBuf, i32>,
}

impl Watches {
    /// Adds `dir` on the watch `wd`. Returns a watch it left that no path
    /// is on any more, for the kernel to drop.
    fn add(&mut self, dir: &Path, wd: i32) -> Option<i32> {
        let old = self.wds.insert(dir.to_owned(), wd);
        if old == Some(wd) {
            return None;
        }
        self.paths.entry(wd).or_default().push(dir.to_owned());
        old.and_then(|old| self.take_off(old, dir))
    }

    /// Takes `dir` off its watch. Returns that watch if no path is on it
    /// any more, for the kernel to drop.
    fn remove(&mut self, dir: &Path) -> Option<i32> {
        let wd = self.wds.remove(dir)?;
        self.take_off(wd, dir)
    }

    /// Takes `dir` off `wd`'s paths (not off `wds`); `wd` once empty.
    fn take_off(&mut self, wd: i32, dir: &Path) -> Option<i32> {
        let paths = self.paths.get_mut(&wd)?;
        paths.retain(|path| path != dir);
        paths.is_empty().then(|| {
            self.paths.remove(&wd);
            wd
        })
    }

    /// The kernel dropped the watch `wd` (its folder went).
    fn ignored(&mut self, wd: i32) {
        for path in self.paths.remove(&wd).unwrap_or_default() {
            if self.wds.get(&path) == Some(&wd) {
                self.wds.remove(&path);
            }
        }
    }
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

    /// How many folders are watched.
    #[cfg(test)]
    pub fn watched(&self) -> usize {
        lock(&self.dirs).wds.len()
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
        let unused = dirs.add(dir, wd);
        self.rm_watches(unused);
        Ok(())
    }

    /// Has the kernel drop the watches no path is on any more. Called under
    /// the `dirs` lock: a `watch_for` of the same folder in between would
    /// get the dropped watch back from the kernel and lose it.
    fn rm_watches(&self, unused: impl IntoIterator<Item = i32>) {
        for wd in unused {
            // SAFETY: no pointers; a stale `wd` only fails with EINVAL.
            unsafe { libc::inotify_rm_watch(self.file.as_raw_fd(), wd) };
        }
    }

    /// Stops watching the folders `keep` says no to, like ones a swapped
    /// link no longer leads to.
    pub fn unwatch_unless(&self, keep: impl Fn(&Path) -> bool) {
        let mut dirs = lock(&self.dirs);
        let dropped: Vec<PathBuf> = dirs.wds.keys().filter(|path| !keep(path)).cloned().collect();
        let unused: Vec<i32> = dropped.iter().filter_map(|path| dirs.remove(path)).collect();
        self.rm_watches(unused);
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
                dirs.ignored(wd);
                continue;
            }
            let paths = dirs.paths.get(&wd).map(Vec::as_slice).unwrap_or_default();
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
        let pollfd = |fd, events| libc::pollfd { fd, events, revents: 0 };
        let mut fds = [pollfd(self.file.as_raw_fd(), libc::POLLIN), pollfd(self.stop.0.as_raw_fd(), libc::POLLIN)];
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
                return Ok(if fds[1].revents != 0 { Woke::Stopped } else { Woke::Changed });
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
    /// Events to read.
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

    /// A tree as wide as a big Home's is watched, re-watched and dropped in
    /// time linear in its folders (IMP-339: each watch used to look at all
    /// the others, which took hours for 173k folders).
    #[test]
    fn a_big_tree_is_watched_quickly() {
        const WIDE: usize = 50;
        const DEEP: usize = 100;
        let dir = nova_test_support::TempDir::new("inotify-big-tree");
        for i in 0..WIDE {
            for j in 0..DEEP {
                std::fs::create_dir_all(dir.join(format!("{i}/{j}"))).unwrap();
            }
        }
        let folders = 1 + WIDE + WIDE * DEEP;
        let watcher = Inotify::new(ENTRIES).unwrap();
        let started = Instant::now();
        // A rescan watches every folder again.
        for _ in 0..2 {
            watcher.watch_tree(&dir).unwrap();
        }
        let took = started.elapsed();
        assert_eq!(watcher.watched(), folders);
        // Quadratic bookkeeping took over 2 s here in a release build;
        // linear takes under 100 ms, most of it the kernel's.
        assert!(took < std::time::Duration::from_secs(1), "watching {folders} folders twice took {took:?}");
        // Only "1" and below go: "10" to "19" start with the same digit.
        let one = dir.join("1");
        watcher.unwatch_unless(|path| !path.starts_with(&one));
        assert_eq!(watcher.watched(), folders - 1 - DEEP);
        watcher.unwatch_unless(|_| false);
        assert_eq!(watcher.watched(), 0);
    }

    /// A link and its folder share the kernel's watch: dropping one path
    /// keeps the watch for the other.
    #[test]
    fn a_shared_watch_lives_until_its_last_path_goes() {
        let dir = nova_test_support::TempDir::new("inotify-shared");
        let folder = dir.join("folder");
        let link = dir.join("link");
        std::fs::create_dir(&folder).unwrap();
        std::os::unix::fs::symlink(&folder, &link).unwrap();
        let watcher = Inotify::new(ENTRIES).unwrap();
        watcher.watch(&folder).unwrap();
        watcher.watch(&link).unwrap();
        watcher.watch(&link).unwrap();
        assert_eq!(watcher.watched(), 2);
        watcher.unwatch_unless(|path| path != link);
        assert_eq!(watcher.watched(), 1);
        std::fs::create_dir(folder.join("new")).unwrap();
        let changes = watcher.read().unwrap().unwrap();
        let paths: Vec<_> = changes.found.iter().map(|change| change.path.clone()).collect();
        assert_eq!(paths, [folder.join("new")]);
    }

    /// Re-watching a path after it names another folder moves it there and
    /// drops the old folder's watch.
    #[test]
    fn a_path_watched_again_follows_what_it_names_now() {
        let dir = nova_test_support::TempDir::new("inotify-swap");
        let (old, new, link) = (dir.join("old"), dir.join("new"), dir.join("link"));
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        std::os::unix::fs::symlink(&old, &link).unwrap();
        let watcher = Inotify::new(ENTRIES).unwrap();
        watcher.watch(&link).unwrap();
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&new, &link).unwrap();
        // The link's own swap lands on no watch: only `link` was watched,
        // through its target.
        watcher.watch(&link).unwrap();
        assert_eq!(watcher.watched(), 1);
        std::fs::create_dir(old.join("unseen")).unwrap();
        std::fs::create_dir(new.join("seen")).unwrap();
        let changes = watcher.read().unwrap().unwrap();
        let paths: Vec<_> = changes.found.iter().map(|change| change.path.clone()).collect();
        assert_eq!(paths, [link.join("seen")]);
    }
}
