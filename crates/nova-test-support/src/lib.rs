//! What Nova's Rust tests share (a dev-dependency only): a temporary folder
//! that cleans up after itself even when the test fails, fake programs to
//! put on a `PATH`, and waiting until something happens. Small helpers, not
//! a fixture framework: each test still builds its own real setup.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// How often [`eventually`] checks again.
const POLL: Duration = Duration::from_millis(10);

/// A fresh, empty folder under the system's temporary folder, removed with
/// everything in it when dropped.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    /// `name` says which test it's for; the folder's name adds this
    /// process and a count, so tests running at once never share one.
    pub fn new(name: &str) -> Self {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let count = COUNT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("nova-test-{name}-{}-{count}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap_or_else(|e| panic!("can't make {}: {e}", path.display()));
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// `relative` inside the folder.
    pub fn join(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.0.join(relative)
    }

    /// The folder as a `PATH`-style string, for a folder of fake programs.
    pub fn path_var(&self) -> String {
        self.0.display().to_string()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Writes an executable `sh` script named `name` into `dir` that runs
/// `body`, and returns its path.
pub fn write_program(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap_or_else(|e| panic!("can't write {name}: {e}"));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Checks `check` until it gives a value or `timeout` passes, then panics
/// with `what` (what the test was waiting for).
pub fn eventually<T>(timeout: Duration, what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < deadline, "waited {} s for: {what}", timeout.as_secs_f32());
        std::thread::sleep(POLL);
    }
}

/// Waits until `check` is true, like [`eventually`].
pub fn wait_until(timeout: Duration, what: &str, mut check: impl FnMut() -> bool) {
    eventually(timeout, what, || check().then_some(()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_temp_dir_is_fresh_and_goes_away() {
        let (first, second) = (TempDir::new("support"), TempDir::new("support"));
        assert_ne!(first.path(), second.path());
        let kept = first.path().to_owned();
        write_program(first.path(), "tool", "exit 0");
        drop(first);
        assert!(!kept.exists());
    }

    #[test]
    fn eventually_gives_the_value_once_it_comes() {
        let mut tries = 0;
        let found = eventually(Duration::from_secs(5), "the third try", || {
            tries += 1;
            (tries == 3).then_some(tries)
        });
        assert_eq!(found, 3);
    }

    #[test]
    #[should_panic(expected = "waited")]
    fn eventually_panics_at_the_deadline() {
        wait_until(Duration::from_millis(30), "never", || false);
    }
}
