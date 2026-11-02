//! Following folders that may not be there yet or sit behind links that
//! get swapped: what to watch so a change is seen ([`WatchPoints`]), and a
//! thread that watches them and says when something that counts changed
//! ([`follow`]). The app index follows `applications/` with it (trees),
//! the MIME database `mime/` (only the folder's own files).
//!
//! A NixOS rebuild or a Nix profile switch swaps a link far above the
//! folder (`/run/current-system`, `~/.local/state/nix/profiles/profile`)
//! and leaves the old folders as they were, so each link on the way there
//! is watched in its folder, and what to watch is worked out again after
//! each change.

use crate::inotify::{self, Inotify};
use crate::logger::Logger;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// Links followed when looking for what to watch, like the kernel's limit.
const MAX_LINKS: usize = 40;
/// Installing a package writes many files; whoever follows them looks
/// again once it pauses this long ([`settled`]).
pub const SETTLE: Duration = Duration::from_millis(500);

/// What to watch so a change to some folders is seen, and the paths whose
/// changes count.
#[derive(Debug, Default, PartialEq)]
pub struct WatchPoints {
    /// Folders whose files and subfolders count (where they resolve to).
    pub trees: BTreeSet<PathBuf>,
    /// Folders whose files count, but not their subfolders.
    pub folders: BTreeSet<PathBuf>,
    /// Folders holding a link on the way to a followed folder, or the
    /// folder it may appear in. Only `entries` count there.
    pub parents: BTreeSet<PathBuf>,
    /// Those links and would-be folders.
    pub entries: BTreeSet<PathBuf>,
}

impl WatchPoints {
    /// Whether a change at `path` counts.
    pub fn counts(&self, path: &Path) -> bool {
        self.entries.contains(path)
            || self.trees.iter().any(|tree| path.starts_with(tree))
            || path.parent().is_some_and(|parent| self.folders.contains(parent))
    }

    /// Whether `dir` is one of the folders to watch, or below a tree.
    pub fn watches(&self, dir: &Path) -> bool {
        self.parents.contains(dir) || self.folders.contains(dir) || self.trees.iter().any(|tree| dir.starts_with(tree))
    }

    /// Adds `folder` and what's below it, links down to `max_depth` folders
    /// deep included, the links on the way to it, or where it would appear.
    pub fn add_tree(&mut self, folder: &Path, max_depth: usize) {
        if let Some(resolved) = self.add(folder) {
            self.linked_folders(&resolved, 0, max_depth);
            self.trees.insert(resolved);
        }
    }

    /// Adds `folder`'s own files, the links on the way to it, or where it
    /// would appear.
    pub fn add_folder(&mut self, folder: &Path) {
        if let Some(resolved) = self.add(folder) {
            self.folders.insert(resolved);
        }
    }

    /// Adds the links on the way to `folder` and its own entry, and gives
    /// where it resolves to; or where it would appear, when it isn't there.
    fn add(&mut self, folder: &Path) -> Option<PathBuf> {
        self.links_on_the_way(folder);
        match std::fs::canonicalize(folder) {
            // Watched through its folder too, so a replacement (one renamed
            // over it, or one made after it was deleted) is seen.
            Ok(resolved) => {
                if let Some(parent) = resolved.parent() {
                    self.parents.insert(parent.to_owned());
                    self.entries.insert(resolved.clone());
                }
                Some(resolved)
            }
            // Not there (yet): the folder it would appear in is watched.
            Err(_) => {
                if let Some(existing) = folder.ancestors().skip(1).find(|dir| dir.is_dir()) {
                    self.parents.insert(existing.to_owned());
                    let child = folder.strip_prefix(existing).ok().and_then(|rest| rest.components().next());
                    self.entries.extend(child.map(|child| existing.join(child)));
                }
                None
            }
        }
    }

    /// Watches them all with `watcher`, telling `logger` what can't be
    /// watched, as `source`, for `what` ("new apps").
    pub fn watch(&self, watcher: &Inotify, logger: &Logger, source: &str, what: &str) {
        // The parents first: a folder that's both gets the trees' wider mask.
        for parent in &self.parents {
            if let Err(e) = watcher.watch_for(parent, inotify::ENTRIES) {
                logger.warn(source, &format!("can't watch {} for {what}: {e}", parent.display()));
            }
        }
        for tree in &self.trees {
            if let Err(e) = watcher.watch_tree(tree) {
                logger.warn(source, &format!("can't watch {} for {what}: {e}", tree.display()));
            }
        }
        for folder in &self.folders {
            if let Err(e) = watcher.watch(folder) {
                logger.warn(source, &format!("can't watch {} for {what}: {e}", folder.display()));
            }
        }
    }

    /// Adds the folders that links below `dir` lead to as trees of their own:
    /// a tree's watches don't follow links, though reading it does.
    fn linked_folders(&mut self, dir: &Path, depth: usize, max_depth: usize) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for path in entries.flatten().map(|entry| entry.path()) {
            if depth >= max_depth || !path.is_dir() {
                continue;
            }
            match path.is_symlink() {
                true => {
                    if let Ok(target) = std::fs::canonicalize(&path)
                        && self.trees.insert(target.clone())
                    {
                        self.linked_folders(&target, depth + 1, max_depth);
                    }
                }
                false => self.linked_folders(&path, depth + 1, max_depth),
            }
        }
    }

    /// Adds each link on the way to `path`, following them, with its folder.
    fn links_on_the_way(&mut self, path: &Path) {
        let mut path = path.to_owned();
        'resolve: for _ in 0..MAX_LINKS {
            let mut prefix = PathBuf::new();
            let mut components = path.components();
            while let Some(component) = components.next() {
                prefix.push(component);
                let Ok(target) = std::fs::read_link(&prefix) else { continue };
                let Some(parent) = prefix.parent() else { return };
                self.parents.insert(parent.to_owned());
                self.entries.insert(prefix.clone());
                path = parent.join(target).join(components.as_path());
                continue 'resolve;
            }
            return;
        }
    }
}

/// What a follower says where.
pub struct Reporting {
    pub logger: Arc<Logger>,
    /// The log source.
    pub source: &'static str,
    /// What it watches for, for the log ("new apps").
    pub what: &'static str,
}

/// Watches what `points` says now, then on a thread of its own sends
/// `changed` for each change that counts, looking again at what to watch
/// after each, until `watcher` is stopped or `changed`'s receiver is gone.
pub fn follow(
    watcher: Inotify,
    points: impl Fn() -> WatchPoints + Send + 'static,
    changed: mpsc::Sender<()>,
    reporting: Reporting,
) {
    let Reporting { logger, source, what } = reporting;
    let watch_all = move |watcher: &Inotify, logger: &Logger| {
        let found = points();
        found.watch(watcher, logger, source, what);
        found
    };
    let mut watched = watch_all(&watcher, &logger);
    std::thread::spawn(move || {
        loop {
            let found = match watcher.read() {
                Ok(Some(found)) => found,
                Ok(None) => return,
                Err(e) => {
                    logger.warn(source, &format!("stopped watching for {what}: {e}"));
                    return;
                }
            };
            if !found.overflowed && !found.found.iter().any(|change| watched.counts(&change.path)) {
                continue;
            }
            // A folder or link may have come or gone: look again at what to
            // watch. The new watches go on before the old ones come off, so
            // nothing goes unwatched meanwhile; dropping those no longer
            // needed keeps a rebuild's leftover store folders from piling up.
            watched = watch_all(&watcher, &logger);
            watcher.unwatch_unless(|dir| watched.watches(dir));
            if changed.send(()).is_err() {
                return;
            }
        }
    });
}

/// Waits for a change from [`follow`], then until they pause for
/// [`SETTLE`]. False once the following ended.
pub fn settled(changes: &mpsc::Receiver<()>) -> bool {
    if changes.recv().is_err() {
        return false;
    }
    while changes.recv_timeout(SETTLE).is_ok() {}
    true
}
