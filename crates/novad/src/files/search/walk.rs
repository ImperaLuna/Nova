//! Reading folders into the index: a whole root at start and on each
//! rescan, or one folder that appeared. Each folder is watched before it's
//! read, so nothing created in between is missed, and read with the types
//! its entries came with (no file is opened or looked up). Links aren't
//! followed, so a link to a folder can't loop; it's listed as a name.
//!
//! What's left out ([`SKIPPED_NAMES`], [`SKIPPED_IN_HOME`], drives' trash) are caches and
//! tool folders full of names nobody looks for, which would bury real
//! results; and mounts the walk doesn't follow ([`Mounts::follows`]).

use super::index::{Id, Index};
use super::roots::Mounts;
use crate::inotify::{Inotify, Stopper};
use std::path::{Path, PathBuf};

/// Folders left out wherever they are: version control internals and
/// package caches of build tools.
pub const SKIPPED_NAMES: [&str; 3] = [".git", "node_modules", "__pycache__"];
/// Folders left out of Home: caches apps rebuild, and the trash.
pub const SKIPPED_IN_HOME: [&str; 2] = [".cache", ".local/share/Trash"];
/// The start of a drive's trash folder's name, `.Trash-<uid>` (or
/// `.Trash` with the user's folder in it).
const TRASH_ON_DRIVES: &str = ".Trash";

/// What a walk needs besides the index.
pub struct Walk<'a> {
    pub watcher: &'a mut Inotify,
    pub mounts: &'a Mounts,
    /// Folders left out by path: [`SKIPPED_IN_HOME`] under Home, and the
    /// roots, each read once as its own (Home on a drive).
    pub skipped: &'a [PathBuf],
    pub stopper: &'a Stopper,
    /// Folders that couldn't be watched, the first time it happened: the
    /// rest is kept fresh by rescans only.
    pub unwatched: Option<std::io::Error>,
}

impl Walk<'_> {
    /// Reads the folder `path` and everything below it into `index` under
    /// `id`. Returns `false` when stopped first.
    pub fn read(&mut self, index: &mut Index, id: Id, path: PathBuf) -> bool {
        let mut folders = vec![(id, path)];
        while let Some((id, folder)) = folders.pop() {
            if self.stopper.stopped() {
                return false;
            }
            // Past the watch limit, the rest waits for rescans. Any other
            // failure means the folder went or can't be read, which reading
            // it shows next.
            if self.unwatched.is_none()
                && let Err(e) = self.watcher.watch(&folder)
                && e.raw_os_error() == Some(libc::ENOSPC)
            {
                self.unwatched = Some(e);
            }
            // A folder that went or can't be read is listed empty.
            let Ok(entries) = std::fs::read_dir(&folder) else { continue };
            let fresh = index.is_empty_folder(id);
            for entry in entries.flatten() {
                let name = nova_protocol::wire_text(entry.file_name());
                let dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
                let child = if fresh { index.add_new(id, &name, dir) } else { index.add(id, &name, dir) };
                // The real name, which may not be UTF-8, so the folder opens.
                let path = entry.path();
                if dir && self.follows(&path) {
                    folders.push((child, path));
                }
            }
        }
        true
    }

    /// Stops watching `folder` and the folders in it, which left the index.
    pub fn forget(&mut self, folder: &Path) {
        self.watcher.unwatch_tree(folder);
    }

    /// Whether the walk goes into the folder `path`.
    pub fn follows(&self, path: &Path) -> bool {
        let name = path.file_name().unwrap_or_default();
        !SKIPPED_NAMES.iter().any(|skipped| name == *skipped)
            && !name.as_encoded_bytes().starts_with(TRASH_ON_DRIVES.as_bytes())
            && !self.skipped.iter().any(|skipped| skipped == path)
            && self.mounts.follows(path)
    }
}

/// [`SKIPPED_IN_HOME`] under `home`.
pub fn skipped_in(home: Option<&Path>) -> Vec<PathBuf> {
    home.map(|home| SKIPPED_IN_HOME.iter().map(|skipped| home.join(skipped)).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::search::matching::Query;
    use nova_test_support::TempDir;

    fn names(index: &Index, query: &str) -> Vec<String> {
        let (hits, _) = index.search(&Query::parse(query).unwrap(), None, 50);
        hits.into_iter().map(|hit| hit.name).collect()
    }

    #[test]
    fn hidden_folders_are_read_and_caches_left_out() {
        let home = TempDir::new("search-walk");
        let game = home.join(".local/share/Steam/steamapps/common/Crimson Desert");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("CrimsonDesert.exe"), b"").unwrap();
        for cache in [
            ".cache/thumbnails",
            ".local/share/Trash/files",
            "code/app/node_modules/crimson",
            "code/.git",
            "usb/.Trash-1000/files",
        ] {
            std::fs::create_dir_all(home.join(cache)).unwrap();
        }
        std::fs::write(home.join(".cache/thumbnails/crimson.png"), b"").unwrap();
        std::fs::write(home.join(".local/share/Trash/files/crimson.txt"), b"").unwrap();
        std::fs::write(home.join("usb/.Trash-1000/files/crimson.txt"), b"").unwrap();
        std::os::unix::fs::symlink(&game, home.join("crimson-link")).unwrap();

        let mut watcher = Inotify::new(crate::inotify::ENTRIES).unwrap();
        let stopper = watcher.stopper();
        let skipped = skipped_in(Some(&home));
        let mounts = Mounts::default();
        let mut walk =
            Walk { watcher: &mut watcher, mounts: &mounts, skipped: &skipped, stopper: &stopper, unwatched: None };
        let mut index = Index::default();
        let root = index.add_root(&home);
        assert!(walk.read(&mut index, root, home.to_path_buf()));
        assert_eq!(names(&index, "crimson"), ["crimson-link", "Crimson Desert", "CrimsonDesert.exe"]);
        // The skipped folders are listed, but not what's in them.
        assert!(index.find(&home.join(".cache")).is_some());
        assert!(index.find(&home.join(".cache/thumbnails")).is_none());
        assert!(watcher.watched() > 5, "every folder read is watched");
    }

    #[test]
    fn a_stopped_walk_ends_early() {
        let home = TempDir::new("search-walk-stop");
        std::fs::create_dir_all(home.join("a/b")).unwrap();
        let mut watcher = Inotify::new(crate::inotify::ENTRIES).unwrap();
        let stopper = watcher.stopper();
        stopper.stop();
        let mounts = Mounts::default();
        let mut walk =
            Walk { watcher: &mut watcher, mounts: &mounts, skipped: &[], stopper: &stopper, unwatched: None };
        let mut index = Index::default();
        let root = index.add_root(&home);
        assert!(!walk.read(&mut index, root, home.to_path_buf()));
    }
}
