//! What Nova's Rust tests share (a dev-dependency only): a temporary folder
//! that cleans up after itself even when the test fails, fake programs to
//! put on a `PATH`, and waiting until something happens. Small helpers, not
//! a fixture framework: each test still builds its own real setup.

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

    /// The folder as a `PATH`-style string, for a folder of fake programs.
    pub fn path_var(&self) -> String {
        self.0.display().to_string()
    }
}

/// The folder's path: `dir.join(..)`, `&dir` where a `&Path` goes.
impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Writes an executable `sh` script named `name` into `dir` that runs
/// `body`, and returns its path.
///
/// The program itself is written by a `sh` child, never through a file this
/// process opens: another test thread forking meanwhile would inherit that
/// write handle, and running the program while the fork still holds it fails
/// with "Text file busy" (ETXTBSY). Only the source text goes through ours,
/// in its own temporary file outside `dir`, so a test watching `dir` never
/// sees it.
pub fn write_program(dir: &Path, name: &str, body: &str) -> PathBuf {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let path = dir.join(name);
    let count = COUNT.fetch_add(1, Ordering::Relaxed);
    let source = std::env::temp_dir().join(format!("nova-test-program-{}-{count}", std::process::id()));
    std::fs::write(&source, format!("#!/bin/sh\n{body}\n")).unwrap_or_else(|e| panic!("can't write {name}: {e}"));
    let copied = std::process::Command::new("/bin/sh")
        .args(["-c", "cat \"$1\" >\"$2\" && chmod 755 \"$2\"", "sh"])
        .args([&source, &path])
        .status()
        .unwrap_or_else(|e| panic!("can't write {name}: {e}"));
    assert!(copied.success(), "can't write {name}: {copied}");
    std::fs::remove_file(&source).unwrap();
    path
}

/// Links each of the host's `tools` (found on this process's `PATH`) into
/// `bin`, beside the test's fake programs, and returns `bin` as the `PATH`
/// to run them with. Only what the test puts there is found, so a program
/// installed on the host can never stand in for one the test means to be
/// missing.
pub fn fake_path(bin: &Path, tools: &[&str]) -> String {
    let host = std::env::var_os("PATH").unwrap_or_default();
    for tool in tools {
        let found = std::env::split_paths(&host)
            .map(|dir| dir.join(tool))
            .find(|path| path.is_file())
            .unwrap_or_else(|| panic!("{tool} isn't on PATH"));
        std::os::unix::fs::symlink(&found, bin.join(tool)).unwrap_or_else(|e| panic!("can't link {tool}: {e}"));
    }
    bin.display().to_string()
}

/// Checks `check` until it gives a value or `timeout` passes, then panics
/// with `what` (what the test was waiting for).
pub fn eventually<T>(timeout: Duration, what: &str, check: impl FnMut() -> Option<T>) -> T {
    found_within(timeout, check).unwrap_or_else(|| panic!("waited {} s for: {what}", timeout.as_secs_f32()))
}

/// What `check` gives once it gives something, or `None` once `timeout`
/// passes without.
pub fn found_within<T>(timeout: Duration, mut check: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = check() {
            return Some(value);
        }
        if Instant::now() >= deadline {
            return None;
        }
        settle(POLL);
    }
}

/// Whether `check` comes true within `timeout`. A `false` took the whole
/// `timeout`, so it also says nothing happened in that time.
pub fn holds_within(timeout: Duration, mut check: impl FnMut() -> bool) -> bool {
    found_within(timeout, || check().then_some(())).is_some()
}

/// Waits out `time`, for a test that checks nothing happens within it, or
/// that paces what it does. The one place tests wait on the clock: a wait
/// for something to happen is [`eventually`], [`found_within`] or
/// [`holds_within`].
#[expect(clippy::disallowed_methods, reason = "tests wait out quiet times and poll here, and only here")]
pub fn settle(time: Duration) {
    std::thread::sleep(time);
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
    fn a_fake_path_holds_only_the_fakes_and_the_tools_asked_for() {
        let dir = TempDir::new("fake-path");
        write_program(&dir, "fake", "sleep 0");
        let path_var = fake_path(&dir, &["sleep"]);
        let mut names: Vec<_> =
            std::fs::read_dir(dir.path()).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        names.sort();
        assert_eq!(names, ["fake", "sleep"]);
        let status = std::process::Command::new(dir.join("fake")).env("PATH", path_var).status().unwrap();
        assert!(status.success());
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
