use nova_base::lock;
use nova_inotify::{self as inotify, Inotify};
use std::path::Path;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Waker {
    wakes: Mutex<Wakes>,
    condvar: Condvar,
}

#[derive(Default)]
struct Wakes {
    stopped: bool,
    plugged: Option<Instant>,
}

impl Waker {
    pub fn plugged(&self) {
        lock(&self.wakes).plugged = Some(Instant::now());
        self.condvar.notify_all();
    }

    pub fn stop(&self) {
        lock(&self.wakes).stopped = true;
        self.condvar.notify_all();
    }

    pub fn wait(&self, next: Instant, settle: Duration) -> bool {
        let mut wakes = lock(&self.wakes);
        loop {
            if wakes.stopped {
                return false;
            }
            let due = wakes.plugged.map_or(next, |plugged| next.min(plugged + settle));
            let now = Instant::now();
            if now >= due {
                wakes.plugged = None;
                return true;
            }
            wakes = self.condvar.wait_timeout(wakes, due - now).unwrap_or_else(std::sync::PoisonError::into_inner).0;
        }
    }
}

pub fn watch(
    dir: &Path,
    changed: impl Fn() + Send + 'static,
    failed: impl FnOnce(std::io::Error) + Send + 'static,
) -> std::io::Result<inotify::Stopper> {
    follow(dir, None, changed, failed)
}

pub fn watch_entries(
    dir: &Path,
    prefix: &'static str,
    changed: impl Fn() + Send + 'static,
    failed: impl FnOnce(std::io::Error) + Send + 'static,
) -> std::io::Result<inotify::Stopper> {
    follow(dir, Some(prefix), changed, failed)
}

fn follow(
    dir: &Path,
    prefix: Option<&'static str>,
    changed: impl Fn() + Send + 'static,
    failed: impl FnOnce(std::io::Error) + Send + 'static,
) -> std::io::Result<inotify::Stopper> {
    let tree = prefix.is_none();
    let watcher = Inotify::new(inotify::ENTRIES)?;
    if tree {
        watcher.watch_tree(dir)?;
    } else {
        watcher.watch_for(dir, inotify::ENTRIES | libc::IN_ATTRIB)?;
    }
    let stopper = watcher.stopper();
    let dir = dir.to_owned();
    std::thread::spawn(move || {
        loop {
            let changes = match watcher.read() {
                Ok(Some(changes)) => changes,
                Ok(None) => return,
                Err(e) => return failed(e),
            };
            if tree
                && (changes.overflowed || changes.found.iter().any(|change| change.is_dir))
                && let Err(e) = watcher.watch_tree(&dir)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                return failed(e);
            }
            let named = |change: &inotify::Change| {
                prefix.is_none_or(|prefix| {
                    change.path.file_name().is_some_and(|name| name.to_string_lossy().starts_with(prefix))
                })
            };
            if changes.overflowed || changes.found.iter().any(named) {
                changed();
            }
        }
    });
    Ok(stopper)
}
