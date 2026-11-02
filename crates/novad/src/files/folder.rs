//! One folder as a view shows it: its entries read from disk
//! ([`read_entry`], [`read_folder`]; types from the name alone, so no
//! file is opened) and the view's order ([`order`]), so the UI never
//! sorts.
//!
//! The order is Windows Explorer's: folders first, then by the sort key,
//! ties by name; names compare case aside and with numbers by value
//! ([`natural`]). It's a total order (raw names break the last ties), so
//! every entry has exactly one place and a binary search finds it.

use crate::mime::MimeDb;
use nova_protocol::{FileEntry, FileSort, FileSortKey};
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// MIME type of a symbolic link whose target is missing.
const BROKEN_LINK_MIME: &str = "inode/symlink";

/// The entry `name` in `folder` as it is now, or `None` once it's gone.
/// A link reports what it points to, when that exists. A name that isn't
/// UTF-8 is listed as best it can be shown (IMP-300).
pub fn read_entry(folder: &Path, name: &OsStr, mime: &MimeDb) -> Option<FileEntry> {
    let path = folder.join(name);
    let name = &*name.to_string_lossy();
    let own = std::fs::symlink_metadata(&path).ok()?;
    let link = own.file_type().is_symlink();
    let target = if link { std::fs::metadata(&path).ok() } else { Some(own) };
    let hidden = is_hidden(name);
    let Some(target) = target else {
        return Some(FileEntry {
            name: name.into(),
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
        name: name.into(),
        dir,
        link,
        hidden,
        mime: mime.type_by_name(name, &target).to_owned(),
        size: (!dir).then_some(target.len()),
        modified_ms: modified_ms(&target),
    })
}

/// Every entry in `folder`, hidden ones only when `hidden`, in no order.
pub fn read_folder(folder: &Path, hidden: bool, mime: &MimeDb) -> std::io::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for found in std::fs::read_dir(folder)? {
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
    Ok(entries)
}

/// A dot file, hidden by default as on every Linux desktop.
pub fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

fn modified_ms(metadata: &Metadata) -> Option<i64> {
    let seconds = metadata.mtime().checked_mul(1000)?;
    seconds.checked_add(metadata.mtime_nsec() / 1_000_000)
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

    fn sorted(sort: FileSort, mut entries: Vec<FileEntry>) -> Vec<String> {
        entries.sort_by(|a, b| order(sort, a, b));
        entries.into_iter().map(|entry| entry.name).collect()
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
        assert_eq!(sorted(BY_NAME, entries.clone()), ["alpha", "Zeta", "a.txt", "b.txt"]);
        let by_size = FileSort { key: FileSortKey::Size, descending: true };
        assert_eq!(sorted(by_size, entries), ["alpha", "Zeta", "a.txt", "b.txt"]);
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
