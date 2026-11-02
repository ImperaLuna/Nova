//! One inotify instance for every open folder view, so many windows and
//! tabs don't use up the user's `fs.inotify.max_user_instances`, which
//! every other program shares.
//!
//! A view follows its folder ([`Watcher::follow`]) and gets a channel of
//! what happened to it. One thread reads the instance and sends each view
//! the names that changed in its folder, or that the folder was moved:
//! itself or any folder above it deleted, moved or renamed over, seen from
//! a watch on each of them. Watches are counted by path, so views on the
//! same folder, or on folders inside each other, share them, and a watch
//! goes when the last view that needs it closes. A view that falls
//! [`BACKLOG`] reads behind drops the rest and reads its folder again, as
//! after a full kernel queue. A folder watched as one
//! view's folder and another's ancestor keeps the folder's wider events
//! until neither needs it.

use crate::inotify::{self, Inotify};
use crate::lock;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// What a view's folder is watched for: entries coming, going, written or
/// touched, and the folder itself going.
const FOLDER: u32 = inotify::ENTRIES
    | libc::IN_MODIFY
    | libc::IN_CLOSE_WRITE
    | libc::IN_ATTRIB
    | inotify::ITSELF
    | libc::IN_ONLYDIR
    | libc::IN_EXCL_UNLINK
    | libc::IN_MASK_ADD;
/// Read batches a view can fall behind by before the rest are dropped and
/// it reads its folder again, so a busy folder can't grow novad's memory
/// while its view is still listing.
const BACKLOG: usize = 64;
/// What the folders above it are watched for: their entries coming and
/// going (one on the path renamed or replaced), and themselves going.
const ANCESTOR: u32 = inotify::ENTRIES | inotify::ITSELF | libc::IN_ONLYDIR | libc::IN_MASK_ADD;

/// The shared instance and the views following folders through it.
pub struct Watcher {
    inotify: Inotify,
    /// How many views need each watched path, as their folder or above it.
    needs: Mutex<HashMap<PathBuf, Need>>,
    routes: Mutex<HashMap<u64, Route>>,
    next_route: AtomicU64,
    /// Its thread still reads the instance.
    reading: AtomicBool,
}

#[derive(Default)]
struct Need {
    folders: usize,
    ancestors: usize,
}

impl Need {
    fn mask(&self) -> u32 {
        if self.folders > 0 { FOLDER } else { ANCESTOR }
    }
}

/// Where one view's notices go.
struct Route {
    folder: PathBuf,
    notices: SyncSender<Notice>,
    /// Notices were dropped for a full backlog.
    dropped: Arc<AtomicBool>,
}

/// What the watcher tells a view.
pub enum Notice {
    Seen(Seen),
    /// The watcher stopped working.
    Failed(String),
}

/// What one read of the instance saw for a view.
#[derive(Default)]
pub struct Seen {
    /// Entries in the folder that may have changed.
    pub names: Vec<OsString>,
    /// The folder, or one above it, went or was replaced.
    pub moved: bool,
    /// Events were lost: look at everything again.
    pub lost: bool,
}

impl Seen {
    fn is_empty(&self) -> bool {
        self.names.is_empty() && !self.moved && !self.lost
    }
}

/// What [`Following::wait`] returned with.
pub enum Wait {
    Notice(Notice),
    /// The deadline passed first.
    Quiet,
    /// The view closed ([`Watcher::unfollow`]) or the watcher stopped.
    Stopped,
}

impl Watcher {
    /// Opens the instance and starts the thread that reads it.
    pub fn start() -> std::io::Result<Arc<Self>> {
        let watcher = Arc::new(Self {
            inotify: Inotify::new(FOLDER)?,
            needs: Mutex::default(),
            routes: Mutex::default(),
            next_route: AtomicU64::new(0),
            reading: AtomicBool::new(true),
        });
        let reader = Arc::clone(&watcher);
        std::thread::Builder::new().name("novad-files-watch".into()).spawn(move || reader.run())?;
        Ok(watcher)
    }

    /// Whether its thread still reads the instance: not once it stopped or
    /// failed, when views need a new one.
    pub fn is_reading(&self) -> bool {
        self.reading.load(Ordering::Acquire)
    }

    /// Ends the thread; views following get `Wait::Stopped`.
    pub fn stop(&self) {
        self.inotify.stopper().stop();
        lock(&self.routes).clear();
    }

    /// Starts following `folder`: nothing is watched until
    /// [`Following::watch`].
    pub fn follow(self: &Arc<Self>, folder: &Path) -> Following {
        let id = self.next_route.fetch_add(1, Ordering::Relaxed);
        let (notices, receiver) = std::sync::mpsc::sync_channel(BACKLOG);
        let dropped = Arc::new(AtomicBool::new(false));
        lock(&self.routes).insert(id, Route { folder: folder.to_owned(), notices, dropped: Arc::clone(&dropped) });
        Following { watcher: Arc::clone(self), id, folder: folder.to_owned(), receiver, dropped, held: false }
    }

    /// Ends the view `id`'s notices: its wait returns `Wait::Stopped`.
    pub fn unfollow(&self, id: u64) {
        lock(&self.routes).remove(&id);
    }

    fn run(&self) {
        self.read_all();
        self.reading.store(false, Ordering::Release);
    }

    fn read_all(&self) {
        loop {
            let changes = match self.inotify.read() {
                Ok(Some(changes)) => changes,
                Ok(None) => return,
                Err(e) => {
                    let error = format!("Can't watch folders: {e}");
                    for route in lock(&self.routes).drain().map(|(_, route)| route) {
                        route.send(Notice::Failed(error.clone()));
                    }
                    return;
                }
            };
            for route in lock(&self.routes).values() {
                let seen = route.seen(&changes);
                if !seen.is_empty() {
                    route.send(Notice::Seen(seen));
                }
            }
        }
    }

    /// Watches `folder` and every folder above it, counting this view.
    /// Each is watched again even when counted, in case its watch went
    /// (the folder deleted) before the views counting it closed.
    fn hold(&self, folder: &Path) -> std::io::Result<()> {
        {
            // Counted first, so a view releasing them meanwhile keeps them.
            let mut needs = lock(&self.needs);
            needs.entry(folder.to_owned()).or_default().folders += 1;
            for ancestor in folder.ancestors().skip(1) {
                needs.entry(ancestor.to_owned()).or_default().ancestors += 1;
            }
        }
        self.watch_down(folder).inspect_err(|_| self.release(folder))
    }

    /// Watches what `folder` and the folders above it name now, after one
    /// of them moved. Fails when `folder` can't be watched any more; a
    /// watch that fails keeps the one before, which other views may share.
    fn refresh(&self, folder: &Path) -> std::io::Result<()> {
        self.watch_down(folder)
    }

    /// Watches the folders above `folder` from the top down, then `folder`,
    /// so one replaced while it's being watched is still seen. The lock
    /// isn't held meanwhile: a stalled mount can block resolving a path,
    /// and mustn't hold up other folders.
    fn watch_down(&self, folder: &Path) -> std::io::Result<()> {
        let masks: Vec<(PathBuf, u32)> = {
            let needs = lock(&self.needs);
            let mut masks: Vec<_> = folder
                .ancestors()
                .map(|path| (path.to_owned(), needs.get(path).map_or(ANCESTOR, Need::mask)))
                .collect();
            masks.reverse();
            masks
        };
        let Some(((folder, _), above)) = masks.split_last() else { return Ok(()) };
        for (ancestor, mask) in above {
            // Best effort: one that can't be listed only isn't followed.
            let _ = self.inotify.watch_for(ancestor, *mask);
        }
        self.inotify.watch_for(folder, FOLDER)
    }

    /// Undoes [`Watcher::hold`].
    fn release(&self, folder: &Path) {
        let mut needs = lock(&self.needs);
        let ancestors = folder.ancestors().skip(1).map(|path| (path, false));
        for (path, is_folder) in std::iter::once((folder, true)).chain(ancestors) {
            let Some(need) = needs.get_mut(path) else { continue };
            let count = if is_folder { &mut need.folders } else { &mut need.ancestors };
            *count = count.saturating_sub(1);
            if need.folders + need.ancestors == 0 {
                needs.remove(path);
                self.inotify.unwatch(path);
            }
        }
    }
}

impl Route {
    /// Queues `notice`, or marks it dropped when the view is that far
    /// behind. A view that's closing has dropped its end.
    fn send(&self, notice: Notice) {
        if let Err(TrySendError::Full(_)) = self.notices.try_send(notice) {
            self.dropped.store(true, Ordering::Release);
        }
    }

    /// What `changes` mean for this view's folder.
    fn seen(&self, changes: &inotify::Changes) -> Seen {
        let mut seen = Seen { lost: changes.overflowed, ..Seen::default() };
        // The folder, or one above it, deleted, moved or unmounted.
        seen.moved = changes.gone.iter().any(|gone| self.folder.starts_with(gone));
        for change in &changes.found {
            if change.path.parent() == Some(&self.folder) {
                seen.names.extend(change.path.file_name().map(ToOwned::to_owned));
            } else if change.mask & inotify::ENTRIES != 0 && self.folder.starts_with(&change.path) {
                // An entry on the way to the folder came or went.
                seen.moved = true;
            }
        }
        seen
    }
}

/// One view's hold on its folder; dropping it lets the watches go.
pub struct Following {
    watcher: Arc<Watcher>,
    id: u64,
    folder: PathBuf,
    receiver: Receiver<Notice>,
    /// Shared with its route.
    dropped: Arc<AtomicBool>,
    held: bool,
}

impl Following {
    /// The id [`Watcher::unfollow`] takes.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Watches the folder, or, after it moved, what its path names now.
    /// Read the folder after this: events from before may be missed.
    pub fn watch(&mut self) -> std::io::Result<()> {
        if self.held {
            return self.watcher.refresh(&self.folder);
        }
        self.watcher.hold(&self.folder)?;
        self.held = true;
        Ok(())
    }

    /// The next notice, waiting until `deadline` (`None`: for good).
    pub fn wait(&self, deadline: Option<Instant>) -> Wait {
        let received = match deadline {
            None => self.receiver.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(deadline) => self.receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())),
        };
        match received {
            // A full backlog always has one more to take, which carries
            // the loss.
            Ok(Notice::Seen(mut seen)) => {
                seen.lost |= self.dropped.swap(false, Ordering::AcqRel);
                Wait::Notice(Notice::Seen(seen))
            }
            Ok(notice) => Wait::Notice(notice),
            Err(RecvTimeoutError::Timeout) => Wait::Quiet,
            Err(RecvTimeoutError::Disconnected) => Wait::Stopped,
        }
    }
}

impl Drop for Following {
    fn drop(&mut self) {
        self.watcher.unfollow(self.id);
        if self.held {
            self.watcher.release(&self.folder);
        }
    }
}
