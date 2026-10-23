//! A thin wrapper over Linux inotify: novad learns about new Codex log lines
//! when they're written instead of checking on a timer.

use std::collections::HashMap;
use std::ffi::{CString, OsStr};
use std::io::Read;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// Appends, new files and folders, and files moved in.
const MASK: u32 = libc::IN_MODIFY | libc::IN_CREATE | libc::IN_MOVED_TO;
/// `struct inotify_event` before its name: wd, mask, cookie, len.
const HEADER_BYTES: usize = 16;
const BUFFER_BYTES: usize = 64 * 1024;

/// Something changed inside a watched folder.
pub struct Change {
    pub path: PathBuf,
    pub is_dir: bool,
}

pub struct Inotify {
    file: std::fs::File,
    dirs: HashMap<i32, PathBuf>,
}

impl Inotify {
    pub fn new() -> std::io::Result<Self> {
        // SAFETY: no pointers; returns a new descriptor or -1.
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
        if fd == -1 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `fd` was just opened and nothing else owns it.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        Ok(Self { file: fd.into(), dirs: HashMap::new() })
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
        use std::os::fd::AsRawFd;
        let path = CString::new(dir.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
        // SAFETY: `path` is NUL-terminated and outlives the call.
        let wd = unsafe { libc::inotify_add_watch(self.file.as_raw_fd(), path.as_ptr(), MASK) };
        if wd == -1 {
            return Err(std::io::Error::last_os_error());
        }
        self.dirs.insert(wd, dir.to_owned());
        Ok(())
    }

    /// Blocks until something changes.
    pub fn read(&mut self) -> std::io::Result<Vec<Change>> {
        let mut buffer = vec![0u8; BUFFER_BYTES];
        let read = self.file.read(&mut buffer)?;
        let mut changes = Vec::new();
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
            if mask & libc::IN_IGNORED != 0 {
                self.dirs.remove(&wd);
                continue;
            }
            let name = name.split(|byte| *byte == 0).next().unwrap_or_default();
            if let (Some(dir), false) = (self.dirs.get(&wd), name.is_empty()) {
                let path = dir.join(OsStr::from_bytes(name));
                changes.push(Change { path, is_dir: mask & libc::IN_ISDIR != 0 });
            }
        }
        Ok(changes)
    }
}
