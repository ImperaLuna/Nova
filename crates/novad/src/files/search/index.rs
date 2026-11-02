//! The index itself: every name under the search's roots, as a tree kept
//! small enough to hold a whole home folder (a million names in a few
//! tens of MB). Names sit end to end in one string; each entry is a few
//! numbers: its name's place in that string, its folder, its first child
//! and next sibling, and flags. A path is rebuilt from the folders up,
//! and only for the results shown.
//!
//! Entries removed leave their name behind in the string and their slot
//! is reused; the rescan that rebuilds the index ([`super::walk`]) drops
//! both.
//!
//! A search ([`Index::search`]) checks every name and keeps the best
//! [`Index::search`]'s `limit` in a heap, ranked by how well the name fits
//! ([`Fit`]), then names outside hidden folders first, then shorter names,
//! then the order they were found in.

use super::matching::{Fit, Query};
use std::collections::BinaryHeap;
use std::path::{Path, PathBuf};

/// An entry's place in [`Index::nodes`].
pub type Id = u32;
/// No entry: a root's folder, a last sibling.
const NONE: Id = Id::MAX;

const DIR: u8 = 1;
/// It or a folder it's in starts with a dot.
const HIDDEN: u8 = 1 << 1;
const ASCII: u8 = 1 << 2;
/// One of the folders searched from; its name is its whole path, and it
/// isn't a result itself.
const ROOT: u8 = 1 << 3;
/// Removed; its slot waits to be reused.
const FREE: u8 = 1 << 4;

#[derive(Clone, Copy)]
struct Node {
    parent: Id,
    first_child: Id,
    next_sibling: Id,
    name_start: u32,
    /// Names are at most 255 bytes, a root's path at most 4096.
    name_len: u16,
    flags: u8,
}

/// A name found, before its file is looked at.
pub struct Hit {
    pub folder: PathBuf,
    pub name: String,
    /// A folder it's in is hidden.
    pub in_hidden_folder: bool,
}

#[derive(Default)]
pub struct Index {
    names: String,
    nodes: Vec<Node>,
    roots: Vec<Id>,
    free: Vec<Id>,
    /// Entries in use, roots aside.
    entries: usize,
}

impl Index {
    /// How many names it holds.
    pub fn len(&self) -> usize {
        self.entries
    }

    /// Bytes it holds, about.
    pub fn bytes(&self) -> usize {
        self.names.capacity() + self.nodes.capacity() * size_of::<Node>() + self.free.capacity() * size_of::<Id>()
    }

    /// Gives back the room kept for more names, once a build is done.
    pub fn shrink(&mut self) {
        self.names.shrink_to_fit();
        self.nodes.shrink_to_fit();
    }

    /// Adds the folder `path` to search from; its entries are added with
    /// [`Index::add`].
    pub fn add_root(&mut self, path: &Path) -> Id {
        let id = self.push(NONE, &path.to_string_lossy(), DIR | ROOT);
        self.roots.push(id);
        id
    }

    /// Adds `name`, a folder when `dir`, in the folder `parent`, unless
    /// it's there already. Returns its id either way.
    pub fn add(&mut self, parent: Id, name: &str, dir: bool) -> Id {
        if let Some(existing) = self.child(parent, name) {
            let node = &mut self.nodes[existing as usize];
            node.flags = if dir { node.flags | DIR } else { node.flags & !DIR };
            return existing;
        }
        self.add_new(parent, name, dir)
    }

    /// Whether the folder `id` has nothing in it.
    pub fn is_empty_folder(&self, id: Id) -> bool {
        self.nodes[id as usize].first_child == NONE
    }

    /// Adds `name` to `parent` without looking for it first: for reading a
    /// folder that was empty, where looking would cost a pass over the
    /// names added so far for each name.
    pub fn add_new(&mut self, parent: Id, name: &str, dir: bool) -> Id {
        let hidden = self.nodes[parent as usize].flags & HIDDEN != 0 || name.starts_with('.');
        let mut flags = if dir { DIR } else { 0 };
        if hidden {
            flags |= HIDDEN;
        }
        if name.is_ascii() {
            flags |= ASCII;
        }
        self.entries += 1;
        self.push(parent, name, flags)
    }

    /// The entry `name` in the folder `parent`.
    pub fn child(&self, parent: Id, name: &str) -> Option<Id> {
        let mut at = self.nodes.get(parent as usize)?.first_child;
        while at != NONE {
            if self.name(at) == name {
                return Some(at);
            }
            at = self.nodes[at as usize].next_sibling;
        }
        None
    }

    /// The entry at the absolute `path`, when it's indexed: a root or
    /// something below one.
    pub fn find(&self, path: &Path) -> Option<Id> {
        self.roots.iter().find_map(|&root| {
            let below = path.strip_prefix(self.name(root)).ok()?;
            below.iter().try_fold(root, |folder, part| self.child(folder, &part.to_string_lossy()))
        })
    }

    /// Removes `id` and everything in it. A root stays, emptied.
    pub fn remove(&mut self, id: Id) {
        let mut stack = vec![self.nodes[id as usize].first_child];
        while let Some(mut at) = stack.pop() {
            while at != NONE {
                let node = self.nodes[at as usize];
                stack.push(node.first_child);
                self.release(at);
                at = node.next_sibling;
            }
        }
        self.nodes[id as usize].first_child = NONE;
        if self.nodes[id as usize].flags & ROOT == 0 {
            self.unlink(id);
            self.release(id);
        }
    }

    /// The names that fit `query`, at most `limit` of them, best first,
    /// and how many fit in all.
    pub fn search(&self, query: &Query, limit: usize) -> (Vec<Hit>, usize) {
        let mut best: BinaryHeap<(Fit, bool, u16, Id)> = BinaryHeap::with_capacity(limit + 1);
        let mut total = 0;
        for (id, node) in (0..).zip(&self.nodes) {
            if node.flags & (ROOT | FREE) != 0 {
                continue;
            }
            let Some(fit) = query.fit(self.name(id), node.flags & ASCII != 0) else { continue };
            total += 1;
            let key = (fit, node.flags & HIDDEN != 0, node.name_len, id);
            if best.len() < limit {
                best.push(key);
            } else if best.peek().is_some_and(|worst| key < *worst) {
                best.pop();
                best.push(key);
            }
        }
        let hits = best.into_sorted_vec().into_iter().map(|(.., id)| self.hit(id)).collect();
        (hits, total)
    }

    fn hit(&self, id: Id) -> Hit {
        let mut folders = Vec::new();
        let parent = self.nodes[id as usize].parent;
        let in_hidden_folder = self.nodes[parent as usize].flags & HIDDEN != 0;
        let mut at = parent;
        while at != NONE {
            folders.push(self.name(at));
            at = self.nodes[at as usize].parent;
        }
        let folder = folders.iter().rev().collect();
        Hit { folder, name: self.name(id).to_owned(), in_hidden_folder }
    }

    fn name(&self, id: Id) -> &str {
        let node = &self.nodes[id as usize];
        let start = node.name_start as usize;
        &self.names[start..start + usize::from(node.name_len)]
    }

    /// A new entry first in `parent`, in a free slot when there's one.
    fn push(&mut self, parent: Id, name: &str, flags: u8) -> Id {
        // Names are at most 255 bytes, so only a string of 4 GB overflows.
        let name_start = u32::try_from(self.names.len()).unwrap_or(u32::MAX);
        self.names.push_str(name);
        let next_sibling = if parent == NONE { NONE } else { self.nodes[parent as usize].first_child };
        let name_len = u16::try_from(name.len()).unwrap_or(u16::MAX);
        let node = Node { parent, first_child: NONE, next_sibling, name_start, name_len, flags };
        let id = match self.free.pop() {
            Some(id) => {
                self.nodes[id as usize] = node;
                id
            }
            None => {
                self.nodes.push(node);
                Id::try_from(self.nodes.len() - 1).unwrap_or(NONE)
            }
        };
        if parent != NONE {
            self.nodes[parent as usize].first_child = id;
        }
        id
    }

    /// Takes `id` out of its folder's children.
    fn unlink(&mut self, id: Id) {
        let Node { parent, next_sibling, .. } = self.nodes[id as usize];
        let mut at = self.nodes[parent as usize].first_child;
        if at == id {
            self.nodes[parent as usize].first_child = next_sibling;
            return;
        }
        while at != NONE {
            let next = self.nodes[at as usize].next_sibling;
            if next == id {
                self.nodes[at as usize].next_sibling = next_sibling;
                return;
            }
            at = next;
        }
    }

    fn release(&mut self, id: Id) {
        let node = &mut self.nodes[id as usize];
        node.flags = FREE;
        node.first_child = NONE;
        self.free.push(id);
        self.entries -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(index: &Index, query: &str) -> Vec<String> {
        let (hits, _) = index.search(&Query::parse(query).unwrap(), 10);
        hits.iter().map(|hit| hit.folder.join(&hit.name).display().to_string()).collect()
    }

    /// `/home/me` with a game in Steam's hidden folder, and a wallpaper.
    fn home() -> Index {
        let mut index = Index::default();
        let home = index.add_root(Path::new("/home/me"));
        let local = index.add(home, ".local", true);
        let steam = index.add(local, "Steam", true);
        let game = index.add(steam, "Crimson Desert", true);
        index.add(game, "CrimsonDesert.exe", false);
        let pictures = index.add(home, "Pictures", true);
        index.add(pictures, "crimson-desert-key-art.jpg", false);
        index
    }

    #[test]
    fn hidden_folders_are_searched_and_rank_after_the_rest() {
        let index = home();
        assert_eq!(
            found(&index, "crimson"),
            [
                "/home/me/Pictures/crimson-desert-key-art.jpg",
                "/home/me/.local/Steam/Crimson Desert",
                "/home/me/.local/Steam/Crimson Desert/CrimsonDesert.exe",
            ]
        );
        let (hits, total) = index.search(&Query::parse("desert").unwrap(), 10);
        assert_eq!(total, 3);
        assert_eq!(hits.iter().map(|hit| hit.in_hidden_folder).collect::<Vec<_>>(), [false, true, true]);
    }

    #[test]
    fn a_whole_name_ranks_first_and_the_limit_keeps_the_best() {
        let mut index = Index::default();
        let root = index.add_root(Path::new("/data"));
        for name in ["steamapps", "my steam", "Steam", "steam.sh", "nothing"] {
            index.add(root, name, false);
        }
        let (hits, total) = index.search(&Query::parse("steam").unwrap(), 2);
        assert_eq!(total, 4);
        assert_eq!(hits.iter().map(|hit| hit.name.as_str()).collect::<Vec<_>>(), ["Steam", "steam.sh"]);
    }

    #[test]
    fn a_removed_folder_takes_its_names_with_it_and_slots_are_reused() {
        let mut index = home();
        let steam = index.find(Path::new("/home/me/.local/Steam")).unwrap();
        let before = index.nodes.len();
        index.remove(steam);
        assert_eq!(found(&index, "crimson"), ["/home/me/Pictures/crimson-desert-key-art.jpg"]);
        assert_eq!(index.len(), 3);
        let local = index.find(Path::new("/home/me/.local")).unwrap();
        index.add(local, "share", true);
        assert_eq!(index.nodes.len(), before, "a freed slot is used again");
        assert!(index.find(Path::new("/home/me/.local/share")).is_some());
        assert!(index.find(Path::new("/home/me/.local/Steam")).is_none());
    }

    #[test]
    fn adding_a_name_twice_keeps_one() {
        let mut index = home();
        let home = index.find(Path::new("/home/me")).unwrap();
        let first = index.add(home, "notes.txt", false);
        assert_eq!(index.add(home, "notes.txt", false), first);
        assert_eq!(found(&index, "notes"), ["/home/me/notes.txt"]);
    }

    #[test]
    fn roots_are_not_results() {
        let index = home();
        assert!(found(&index, "home").is_empty());
        assert!(index.find(Path::new("/elsewhere")).is_none());
    }
}
