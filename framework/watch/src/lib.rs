use nova_inotify::{self as inotify, Inotify};
use nova_log::Logger;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

// The kernel's own limit on links followed: https://man7.org/linux/man-pages/man7/path_resolution.7.html
const MAX_LINKS: usize = 40;
pub const SETTLE: Duration = Duration::from_millis(500);

#[derive(Debug, Default, PartialEq)]
pub struct WatchPoints {
    pub trees: BTreeSet<PathBuf>,
    pub folders: BTreeSet<PathBuf>,
    pub parents: BTreeSet<PathBuf>,
    pub entries: BTreeSet<PathBuf>,
}

impl WatchPoints {
    pub fn counts(&self, path: &Path) -> bool {
        self.entries.contains(path)
            || self.trees.iter().any(|tree| path.starts_with(tree))
            || path.parent().is_some_and(|parent| self.folders.contains(parent))
    }

    pub fn watches(&self, dir: &Path) -> bool {
        self.parents.contains(dir) || self.folders.contains(dir) || self.trees.iter().any(|tree| dir.starts_with(tree))
    }

    pub fn add_tree(&mut self, folder: &Path, max_depth: usize) {
        if let Some(resolved) = self.add(folder) {
            self.linked_folders(&resolved, 0, max_depth);
            self.trees.insert(resolved);
        }
    }

    pub fn add_folder(&mut self, folder: &Path) {
        if let Some(resolved) = self.add(folder) {
            self.folders.insert(resolved);
        }
    }

    fn add(&mut self, folder: &Path) -> Option<PathBuf> {
        self.links_on_the_way(folder);
        match std::fs::canonicalize(folder) {
            Ok(resolved) => {
                if let Some(parent) = resolved.parent() {
                    self.parents.insert(parent.to_owned());
                    self.entries.insert(resolved.clone());
                }
                Some(resolved)
            }
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

    pub fn watch(&self, watcher: &Inotify, logger: &Logger, source: &str, what: &str) {
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

pub struct Reporting {
    pub logger: Arc<Logger>,
    pub source: &'static str,
    pub what: &'static str,
}

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
            watched = watch_all(&watcher, &logger);
            watcher.unwatch_unless(|dir| watched.watches(dir));
            if changed.send(()).is_err() {
                return;
            }
        }
    });
}

pub fn settled(changes: &mpsc::Receiver<()>) -> bool {
    if changes.recv().is_err() {
        return false;
    }
    while changes.recv_timeout(SETTLE).is_ok() {}
    true
}
