//! Changing a folder from Nova Files' command row and right-click menu:
//! making a new folder (`files_new_folder`), renaming an item in place
//! (`files_rename`), and what the details pane shows about one
//! (`files_details`). Paths come from the socket as wire text and are
//! checked like a window's ([`super::target::clean`]); a new name must be
//! one name, never a path, so nothing leaves the folder it's in.
//!
//! A rename never replaces another item: it's one `renameat2` with
//! `RENAME_NOREPLACE`, so an item that appears under the new name meanwhile
//! is kept, not lost. On a file system that ignores case (a FAT USB stick),
//! "photo" to "Photo" finds itself under the new name; that one is renamed
//! all the same.

use super::folder::{self, read_entry};
use super::target::clean;
use crate::mime::MimeDb;
use nova_protocol::{FILES_NAME_MAX_BYTES, FileDetails, wire_path, wire_text};
use std::ffi::{CString, OsStr};
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// What a new folder is called before it's renamed, as in Explorer.
const NEW_FOLDER: &str = "New folder";
/// Tries at a free name, in case other apps keep taking the one found.
const NEW_FOLDER_TRIES: usize = 8;

/// Makes "New folder" (or the first free "New folder (n)") in `folder` and
/// answers with its name.
pub fn new_folder(folder: &str) -> Result<String, String> {
    let folder = clean(folder)?;
    let mut name = OsStr::new(NEW_FOLDER).to_os_string();
    for _ in 0..NEW_FOLDER_TRIES {
        match fs::create_dir(folder.join(&name)) {
            Ok(()) => return Ok(wire_text(&name)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                name = super::copying::numbered(&folder, OsStr::new(NEW_FOLDER), true)
                    .map_err(|e| failed("making a folder in", &folder, &e))?;
            }
            Err(e) => return Err(failed("making a folder in", &folder, &e)),
        }
    }
    Err(format!("{} keeps taking every new folder name", folder.display()))
}

/// Renames the item at `path` to `name` (wire text) in its folder, and
/// answers with the new name. Refused when another item has that name.
pub fn rename(path: &str, name: &str) -> Result<String, String> {
    let from = clean(path)?;
    let folder = from.parent().ok_or_else(|| format!("{} can't be renamed", from.display()))?;
    let name = checked_name(name)?;
    let to = folder.join(&name);
    if to == from {
        return Ok(wire_text(&name));
    }
    match rename_no_replace(&from, &to) {
        Ok(()) => Ok(wire_text(&name)),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists && only_case_differs(&from, &to) => {
            fs::rename(&from, &to).map(|()| wire_text(&name)).map_err(|e| failed("renaming", &from, &e))
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            Err(format!("There is already an item named {} here.", wire_text(&name)))
        }
        Err(e) => Err(failed("renaming", &from, &e)),
    }
}

/// The item at `path` as the details pane shows it.
pub fn details(path: &str, mime: &MimeDb) -> Result<FileDetails, String> {
    let path = clean(path)?;
    let (folder, name) = match (path.parent(), path.file_name()) {
        (Some(folder), Some(name)) => (folder, name),
        // The root is a folder like any other, named "/".
        _ => (Path::new("/"), OsStr::new("/")),
    };
    let entry = read_entry(folder, name, mime).ok_or_else(|| format!("{} isn't there", path.display()))?;
    // A link shows what it points to, as its entry does.
    let meta =
        fs::metadata(&path).or_else(|_| fs::symlink_metadata(&path)).map_err(|e| failed("reading", &path, &e))?;
    let items = entry
        .dir
        .then(|| fs::read_dir(&path).ok())
        .flatten()
        .map(|entries| u32::try_from(entries.count()).unwrap_or(u32::MAX));
    Ok(FileDetails {
        entry,
        items,
        created_ms: meta
            .created()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .and_then(|since| i64::try_from(since.as_millis()).ok()),
        accessed_ms: folder::millis(meta.atime(), meta.atime_nsec()),
        owner: crate::host::user_name_of(meta.uid()).unwrap_or_else(|| meta.uid().to_string()),
        group: crate::host::group_name_of(meta.gid()).unwrap_or_else(|| meta.gid().to_string()),
        mode: meta.mode() & 0o777,
    })
}

/// `name` once it's one name: not empty, no `/`, not `.` or `..`, and no
/// longer than a file system takes. Spaces around it are dropped, as in
/// Explorer.
fn checked_name(name: &str) -> Result<PathBuf, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return Err("Type a name.".into());
    }
    if trimmed.contains('/') || trimmed.contains('\0') {
        return Err("A name can't contain /.".into());
    }
    let name = wire_path(trimmed);
    if name.as_os_str().len() > FILES_NAME_MAX_BYTES {
        return Err("That name is too long.".into());
    }
    Ok(name)
}

/// `to` is `from` itself under a name that differs only in case, as a
/// file system that ignores case finds it (not a hard link to it).
fn only_case_differs(from: &Path, to: &Path) -> bool {
    let lower = |path: &Path| path.file_name().map(|name| name.to_string_lossy().to_lowercase());
    let same_name = lower(from) == lower(to);
    match (fs::symlink_metadata(from), fs::symlink_metadata(to)) {
        (Ok(a), Ok(b)) => same_name && a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// `rename`, failing with `AlreadyExists` instead of replacing an item. On
/// a file system without `RENAME_NOREPLACE` (NFS, most FUSE drives) it
/// checks the name is free first, which another app could still beat.
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    match rename_no_replace_flag(from, to) {
        Err(e) if matches!(e.raw_os_error(), Some(libc::EINVAL | libc::ENOSYS)) => match fs::symlink_metadata(to) {
            Ok(_) => Err(io::ErrorKind::AlreadyExists.into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => fs::rename(from, to),
            Err(e) => Err(e),
        },
        done => done,
    }
}

fn rename_no_replace_flag(from: &Path, to: &Path) -> io::Result<()> {
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    // SAFETY: both are NUL-terminated paths that live through the call.
    let status = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if status == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

fn failed(doing: &str, path: &Path, error: &io::Error) -> String {
    format!("{doing} {}: {error}", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    fn text(path: &Path) -> String {
        path.display().to_string()
    }

    #[test]
    fn a_new_folder_takes_the_first_free_number() {
        let dir = TempDir::new("files-new-folder");
        assert_eq!(new_folder(&text(&dir)).unwrap(), "New folder");
        assert_eq!(new_folder(&text(&dir)).unwrap(), "New folder (2)");
        assert!(dir.join("New folder (2)").is_dir());
        assert!(new_folder(&text(&dir.join("missing"))).unwrap_err().contains("missing"));
        assert!(new_folder("relative").is_err(), "only absolute folders");
    }

    #[test]
    fn a_rename_never_replaces_another_item() {
        let dir = TempDir::new("files-rename");
        fs::write(dir.join("a.txt"), "a").unwrap();
        fs::write(dir.join("b.txt"), "b").unwrap();
        let error = rename(&text(&dir.join("a.txt")), "b.txt").unwrap_err();
        assert_eq!(error, "There is already an item named b.txt here.");
        assert_eq!(fs::read_to_string(dir.join("b.txt")).unwrap(), "b", "kept");
        assert_eq!(rename(&text(&dir.join("a.txt")), "  c.txt ").unwrap(), "c.txt", "spaces around go");
        assert_eq!(fs::read_to_string(dir.join("c.txt")).unwrap(), "a");
        assert_eq!(rename(&text(&dir.join("c.txt")), "c.txt").unwrap(), "c.txt", "the same name is no change");
        fs::hard_link(dir.join("c.txt"), dir.join("d.txt")).unwrap();
        assert!(rename(&text(&dir.join("c.txt")), "d.txt").is_err(), "a hard link is another item");
        assert!(dir.join("c.txt").exists());
    }

    #[test]
    fn a_new_name_is_one_name_inside_the_folder() {
        let dir = TempDir::new("files-rename-names");
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("a.txt"), "a").unwrap();
        let from = text(&dir.join("a.txt"));
        for bad in ["", "  ", ".", "..", "sub/a.txt", "../a.txt"] {
            assert!(rename(&from, bad).is_err(), "{bad:?} is refused");
        }
        assert!(rename(&from, &"x".repeat(FILES_NAME_MAX_BYTES + 1)).unwrap_err().contains("too long"));
        assert!(dir.join("a.txt").exists(), "still where it was");
        assert!(rename(&text(&dir.join("sub/../a.txt")), "b.txt").is_err(), "no .. in the path");
    }

    #[test]
    fn details_count_a_folders_items_and_name_its_owner() {
        let dir = TempDir::new("files-details");
        fs::create_dir(dir.join("Trips")).unwrap();
        fs::write(dir.join("Trips/one.txt"), "1").unwrap();
        fs::write(dir.join("Trips/.two"), "2").unwrap();
        fs::write(dir.join("notes.txt"), "hello").unwrap();
        let mime = MimeDb::default();
        let trips = details(&text(&dir.join("Trips")), &mime).unwrap();
        assert_eq!((trips.entry.name.as_str(), trips.entry.dir, trips.items), ("Trips", true, Some(2)));
        let notes = details(&text(&dir.join("notes.txt")), &mime).unwrap();
        assert_eq!((notes.entry.size, notes.items), (Some(5), None));
        assert!(notes.accessed_ms.is_some());
        assert!(!notes.owner.is_empty() && !notes.group.is_empty());
        assert_eq!(notes.mode & 0o600, 0o600, "the test made it, so it reads and writes it");
        assert!(details(&text(&dir.join("gone")), &mime).is_err());
    }
}
