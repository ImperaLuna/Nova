//! One folder as a view shows it: its entries read from disk
//! ([`read_entry`], [`read_folder`]; types from the name alone, so no
//! file is opened), kept in the view's order
//! ([`Listing`]), and each change to an entry turned into the index
//! operations the UI applies to its list ([`Listing::apply`]), so the UI
//! never sorts or searches.
//!
//! The order is Windows Explorer's: folders first, then by the sort key,
//! ties by name; names compare case aside and with numbers by value
//! ([`natural`]). It's a total order (raw names break the last ties), so
//! every entry has exactly one place and a binary search finds it.

use crate::mime::MimeDb;
use nova_protocol::{FileEntry, FileSort, FileSortKey, FolderChange};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// MIME type of a symbolic link whose target is missing.
const BROKEN_LINK_MIME: &str = "inode/symlink";

/// The entry `name` in `folder` as it is now, or `None` once it's gone.
/// A link reports what it points to, when that exists. The name is wire
/// text ([`entry_name`]), so one that isn't UTF-8 still names its file.
pub fn read_entry(folder: &Path, name: &OsStr, mime: &MimeDb) -> Option<FileEntry> {
    let path = folder.join(name);
    let wire = entry_name(name);
    let own = std::fs::symlink_metadata(&path).ok()?;
    let link = own.file_type().is_symlink();
    let target = if link { std::fs::metadata(&path).ok() } else { Some(own) };
    let hidden = is_hidden(&wire);
    let Some(target) = target else {
        return Some(FileEntry {
            name: wire,
            dir: false,
            link,
            hidden,
            mime: BROKEN_LINK_MIME.into(),
            size: None,
            modified_ms: None,
        });
    };
    let dir = target.is_dir();
    Some(FileEntry {
        mime: mime.type_by_name(&name.to_string_lossy(), &target).to_owned(),
        name: wire,
        dir,
        link,
        hidden,
        size: (!dir).then_some(target.len()),
        modified_ms: modified_ms(&target),
    })
}

/// Every entry in `folder`, hidden ones only when `hidden`, in no order.
/// `None` once `stopped` says the entries are no longer wanted, which is
/// asked before each one, so leaving a big folder doesn't finish reading it.
pub fn read_folder(
    folder: &Path,
    hidden: bool,
    mime: &MimeDb,
    stopped: impl Fn() -> bool,
) -> std::io::Result<Option<Vec<FileEntry>>> {
    let mut entries = Vec::new();
    for found in std::fs::read_dir(folder)? {
        if stopped() {
            return Ok(None);
        }
        // An entry deleted while the folder is read is simply not listed.
        let Ok(found) = found else { continue };
        let name = found.file_name();
        if !hidden && name.as_encoded_bytes().starts_with(b".") {
            continue;
        }
        if let Some(entry) = read_entry(folder, &name, mime) {
            entries.push(entry);
        }
    }
    Ok((!stopped()).then_some(entries))
}

/// `name` as an entry's name on the wire, which names it losslessly.
pub fn entry_name(name: &OsStr) -> String {
    nova_protocol::path_to_wire(name.as_encoded_bytes())
}

/// A dot file, hidden by default as on every Linux desktop.
pub fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

fn modified_ms(metadata: &Metadata) -> Option<i64> {
    let seconds = metadata.mtime().checked_mul(1000)?;
    seconds.checked_add(metadata.mtime_nsec() / 1_000_000)
}

/// A folder's entries in a view's order, and where each one is.
pub struct Listing {
    sort: FileSort,
    /// Shown hidden entries.
    hidden: bool,
    /// In the view's order.
    entries: Vec<FileEntry>,
    /// The same entries by name (wire text, so two names that look alike
    /// are two keys), to find an entry's place before it changed.
    by_name: HashMap<String, FileEntry>,
}

impl Listing {
    pub fn new(sort: FileSort, hidden: bool, mut entries: Vec<FileEntry>) -> Self {
        entries.sort_by(|a, b| order(sort, a, b));
        let by_name = entries.iter().map(|entry| (entry.name.clone(), entry.clone())).collect();
        Self { sort, hidden, entries, by_name }
    }

    pub fn entries(&self) -> &[FileEntry] {
        &self.entries
    }

    /// The entry `name` (wire text) is now `now` (`None`: gone). Returns the changes
    /// that take the view's list there, none when it looks the same.
    pub fn apply(&mut self, name: &str, now: Option<FileEntry>) -> Vec<FolderChange> {
        let now = now.filter(|entry| self.hidden || !entry.hidden);
        let before = self.by_name.get(name).map(|entry| self.place(entry));
        match (before, now) {
            (None, None) => Vec::new(),
            (Some(index), None) => {
                self.entries.remove(index);
                self.by_name.remove(name);
                vec![FolderChange::Remove { index: wire_index(index) }]
            }
            (None, Some(entry)) => vec![self.insert(entry)],
            (Some(index), Some(entry)) if self.entries[index] == entry => Vec::new(),
            (Some(index), Some(entry)) if self.keeps_place(index, &entry) => {
                self.entries[index] = entry.clone();
                self.by_name.insert(entry.name.clone(), entry.clone());
                vec![FolderChange::Update { index: wire_index(index), entry }]
            }
            (Some(index), Some(entry)) => {
                self.entries.remove(index);
                vec![FolderChange::Remove { index: wire_index(index) }, self.insert(entry)]
            }
        }
    }

    /// Where `entry`, which is in the list, sits.
    fn place(&self, entry: &FileEntry) -> usize {
        self.entries.binary_search_by(|probe| order(self.sort, probe, entry)).unwrap_or_else(|index| index)
    }

    fn insert(&mut self, entry: FileEntry) -> FolderChange {
        let index = self.entries.partition_point(|probe| order(self.sort, probe, &entry) == Ordering::Less);
        self.entries.insert(index, entry.clone());
        self.by_name.insert(entry.name.clone(), entry.clone());
        FolderChange::Insert { index: wire_index(index), entry }
    }

    /// Whether `entry`, replacing the one at `index`, stays between its
    /// neighbours.
    fn keeps_place(&self, index: usize, entry: &FileEntry) -> bool {
        let after_previous = index == 0 || order(self.sort, &self.entries[index - 1], entry) == Ordering::Less;
        let before_next =
            self.entries.get(index + 1).is_none_or(|next| order(self.sort, entry, next) == Ordering::Less);
        after_previous && before_next
    }
}

/// An index on the wire. A folder with more than `u32::MAX` entries can't
/// be read into memory, let alone listed.
fn wire_index(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

/// The view's order between two entries.
pub fn order(sort: FileSort, a: &FileEntry, b: &FileEntry) -> Ordering {
    let by_key = match sort.key {
        FileSortKey::Name => Ordering::Equal,
        FileSortKey::Kind => a.mime.cmp(&b.mime),
        FileSortKey::Size => a.size.cmp(&b.size),
        FileSortKey::Modified => a.modified_ms.cmp(&b.modified_ms),
    };
    let by_name = natural(&a.name, &b.name);
    // Descending reverses the key; names break ties A to Z, unless the
    // names are the key.
    let within = match (sort.descending, sort.key) {
        (true, FileSortKey::Name) => by_name.reverse(),
        (true, _) => by_key.reverse().then(by_name),
        (false, _) => by_key.then(by_name),
    };
    b.dir.cmp(&a.dir).then(within).then_with(|| a.name.cmp(&b.name))
}

/// Names the way people read them: case aside, and runs of digits by their
/// value, so "file 2" comes before "file 10".
pub fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (x, y) = (digits(&mut a), digits(&mut b));
                let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let by_value = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if by_value != Ordering::Equal {
                    return by_value;
                }
            }
            (Some(x), Some(y)) => {
                let by_letter = x.to_lowercase().cmp(y.to_lowercase());
                if by_letter != Ordering::Equal {
                    return by_letter;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// The run of ASCII digits at the front of `chars`, taken off it.
fn digits(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut run = String::new();
    while let Some(digit) = chars.next_if(char::is_ascii_digit) {
        run.push(digit);
    }
    run
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, size: u64) -> FileEntry {
        FileEntry {
            name: name.into(),
            dir: false,
            link: false,
            hidden: is_hidden(name),
            mime: "text/plain".into(),
            size: Some(size),
            modified_ms: Some(0),
        }
    }

    fn folder(name: &str) -> FileEntry {
        FileEntry { dir: true, size: None, mime: "inode/directory".into(), ..file(name, 0) }
    }

    fn names(listing: &Listing) -> Vec<&str> {
        listing.entries().iter().map(|entry| entry.name.as_str()).collect()
    }

    const BY_NAME: FileSort = FileSort { key: FileSortKey::Name, descending: false };

    #[test]
    fn names_compare_case_aside_and_numbers_by_value() {
        let mut list = ["file 10", "File 2", "file 1", "a", "B", "file 02b", "file 2a"];
        list.sort_by(|a, b| natural(a, b));
        assert_eq!(list, ["a", "B", "file 1", "File 2", "file 2a", "file 02b", "file 10"]);
    }

    #[test]
    fn folders_come_first_and_descending_keeps_them_there() {
        let entries = vec![file("b.txt", 5), folder("Zeta"), file("a.txt", 9), folder("alpha")];
        let listing = Listing::new(BY_NAME, false, entries.clone());
        assert_eq!(names(&listing), ["alpha", "Zeta", "a.txt", "b.txt"]);
        let by_size = FileSort { key: FileSortKey::Size, descending: true };
        let listing = Listing::new(by_size, false, entries);
        assert_eq!(names(&listing), ["alpha", "Zeta", "a.txt", "b.txt"]);
    }

    /// Each change's ops, applied to the UI's copy in order, give the
    /// listing's own list.
    #[test]
    fn changes_replay_to_the_same_list() {
        let by_size = FileSort { key: FileSortKey::Size, descending: false };
        let mut listing = Listing::new(by_size, false, vec![file("a", 1), file("b", 2), file("c", 3)]);
        let mut copy = listing.entries().to_vec();
        let steps = [
            ("d", Some(file("d", 0))),
            ("b", Some(file("b", 2))),
            ("a", Some(file("a", 10))),
            ("c", Some(file("c", 4))),
            ("d", None),
            (".hidden", Some(file(".hidden", 1))),
            ("ghost", None),
        ];
        let mut counts = Vec::new();
        for (name, now) in steps {
            let changes = listing.apply(name, now);
            counts.push(changes.len());
            for change in changes {
                match change {
                    FolderChange::Insert { index, entry } => copy.insert(index as usize, entry),
                    FolderChange::Remove { index } => {
                        copy.remove(index as usize);
                    }
                    FolderChange::Update { index, entry } => copy[index as usize] = entry,
                }
            }
            assert_eq!(copy, listing.entries(), "after {name}");
        }
        // New, unchanged, moved, changed in place, gone, hidden, never there.
        assert_eq!(counts, [1, 0, 2, 1, 1, 0, 0]);
        assert_eq!(names(&listing), ["b", "c", "a"]);
    }

    #[test]
    fn a_broken_link_is_listed_as_a_link() {
        let dir = nova_test_support::TempDir::new("files-broken-link");
        std::os::unix::fs::symlink(dir.join("missing"), dir.join("dangling")).unwrap();
        let entry = read_entry(&dir, OsStr::new("dangling"), &MimeDb::default()).unwrap();
        assert!(entry.link && !entry.dir);
        assert_eq!(entry.mime, BROKEN_LINK_MIME);
    }

    #[test]
    fn a_link_to_a_folder_is_a_folder() {
        let dir = nova_test_support::TempDir::new("files-folder-link");
        std::fs::create_dir(dir.join("real")).unwrap();
        std::os::unix::fs::symlink(dir.join("real"), dir.join("shortcut")).unwrap();
        let entry = read_entry(&dir, OsStr::new("shortcut"), &MimeDb::default()).unwrap();
        assert!(entry.link && entry.dir);
        assert_eq!(entry.size, None);
    }
}
