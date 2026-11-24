#![expect(clippy::panic, clippy::unwrap_used, reason = "test helpers fail the test that calls them, loudly")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
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

    pub fn path_var(&self) -> String {
        self.0.display().to_string()
    }
}

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

pub fn eventually<T>(timeout: Duration, what: &str, check: impl FnMut() -> Option<T>) -> T {
    found_within(timeout, check).unwrap_or_else(|| panic!("waited {} s for: {what}", timeout.as_secs_f32()))
}

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

pub fn holds_within(timeout: Duration, mut check: impl FnMut() -> bool) -> bool {
    found_within(timeout, || check().then_some(())).is_some()
}

#[expect(clippy::disallowed_methods, reason = "tests wait out quiet times and poll here, and only here")]
pub fn settle(time: Duration) {
    std::thread::sleep(time);
}

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
