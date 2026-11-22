//! Devices coming and going, from their device nodes: the kernel adds and
//! removes `/dev/bus/usb/<bus>/<device>` as USB devices are plugged in and
//! out, and `/dev/hidraw<n>` as HID devices are, so watching those folders
//! with inotify costs nothing while nothing changes, and needs neither
//! udev nor netlink. A USB bus that appears later (a new controller) is
//! watched from then on.

use nova_base::lock;
use nova_inotify::{self as inotify, Inotify};
use std::path::Path;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Wakes a thread that reads devices again on a timer, and soon after they
/// come or go, until stopped.
#[derive(Default)]
pub struct Waker {
    wakes: Mutex<Wakes>,
    condvar: Condvar,
}

#[derive(Default)]
struct Wakes {
    stopped: bool,
    /// When a device last came or went, until they are read again.
    plugged: Option<Instant>,
}

impl Waker {
    /// A device came or went.
    pub fn plugged(&self) {
        lock(&self.wakes).plugged = Some(Instant::now());
        self.condvar.notify_all();
    }

    pub fn stop(&self) {
        lock(&self.wakes).stopped = true;
        self.condvar.notify_all();
    }

    /// Waits until `next`, or until `settle` after a device last came or
    /// went if that's sooner. False once stopped.
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

/// Calls `changed` on a thread of its own whenever a USB device is plugged
/// in or out under `dir` (`/dev/bus/usb`), until the returned stopper is
/// used, or until watching fails, which `failed` is told. Fails at once
/// when `dir` can't be watched (it's missing in a container).
pub fn watch(
    dir: &Path,
    changed: impl Fn() + Send + 'static,
    failed: impl FnOnce(std::io::Error) + Send + 'static,
) -> std::io::Result<inotify::Stopper> {
    follow(dir, None, changed, failed)
}

/// Like [`watch`], for the entries of `dir` itself whose names start with
/// `prefix` (`/dev`'s `hidraw` nodes), not the folders in it: they come,
/// go, or have their access changed (udev giving the user its ACL, a
/// moment after the node appears).
pub fn watch_entries(
    dir: &Path,
    prefix: &'static str,
    changed: impl Fn() + Send + 'static,
    failed: impl FnOnce(std::io::Error) + Send + 'static,
) -> std::io::Result<inotify::Stopper> {
    follow(dir, Some(prefix), changed, failed)
}

/// Follows `dir`'s tree, or with `prefix` only those entries of its own.
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
            // A new bus, or lost events: watch every bus folder again
            // (watching one twice is harmless).
            // A bus folder that went again in between (a dock unplugged)
            // is no reason to stop.
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
