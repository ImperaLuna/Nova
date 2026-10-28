//! USB devices coming and going, from their device nodes: the kernel adds
//! and removes `/dev/bus/usb/<bus>/<device>` as they're plugged in and out,
//! so watching those folders with inotify costs nothing while nothing
//! changes, and needs neither udev nor netlink. A bus that appears later
//! (a new controller) is watched from then on.

use crate::inotify::{self, Inotify};
use std::path::Path;

/// Calls `changed` on a thread of its own whenever a USB device is plugged
/// in or out under `dir` (`/dev/bus/usb`), until the returned stopper is
/// used, or until watching fails, which `failed` is told. Fails at once
/// when `dir` can't be watched (it's missing in a container).
pub fn watch(
    dir: &Path,
    changed: impl Fn() + Send + 'static,
    failed: impl FnOnce(std::io::Error) + Send + 'static,
) -> std::io::Result<inotify::Stopper> {
    let mut watcher = Inotify::new(inotify::ENTRIES)?;
    watcher.watch_tree(dir)?;
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
            if (changes.overflowed || changes.found.iter().any(|change| change.is_dir))
                && let Err(e) = watcher.watch_tree(&dir)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                return failed(e);
            }
            changed();
        }
    });
    Ok(stopper)
}
