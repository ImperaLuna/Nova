//! The Properties window's facts about one item (`files_properties`):
//! its listing entry with its type read from its first bytes, its times,
//! owner and who may read, change and run it, and, where a drive is
//! mounted, how full it is. Also changing who may do what
//! (`files_set_access`), for its owner only.
//!
//! A link is described by what it points to, as the listing does, plus
//! where it points; one whose target is gone by the link itself.

use crate::files::folder::{disk_bytes, millis, read_entry};
use crate::files::views::checked_path;
use crate::mime::MimeDb;
use nova_protocol::{Access, DriveSpace, FileProperties, Rights};
use std::fs::{self, Metadata};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

/// The mode bits of each kind of person, from the owner's down.
const OWNER_SHIFT: u32 = 6;
const GROUP_SHIFT: u32 = 3;
const OTHERS_SHIFT: u32 = 0;
const READ_BIT: u32 = 0o4;
const WRITE_BIT: u32 = 0o2;
const RUN_BIT: u32 = 0o1;
/// The bits [`Access`] covers; the rest (setuid, sticky) are kept.
const ACCESS_BITS: u32 = 0o777;

/// What the Properties window shows about the item at `path` (wire text).
pub fn read(path: &str, mime: &MimeDb) -> Result<FileProperties, String> {
    let path = checked_path(path)?;
    let unreadable = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::NotFound => format!("{} doesn't exist", path.display()),
        _ => format!("Can't read {}: {e}", path.display()),
    };
    let own = fs::symlink_metadata(&path).map_err(unreadable)?;
    let (folder, name) = match (path.parent(), path.file_name()) {
        (Some(folder), Some(name)) => (folder, name),
        // The root: listed as itself, in no folder.
        _ => (Path::new(""), path.as_os_str()),
    };
    let mut entry = read_entry(folder, name, mime).ok_or_else(|| format!("{} doesn't exist", path.display()))?;
    let link_target = own.file_type().is_symlink().then(|| fs::read_link(&path).ok()).flatten();
    // What it points to, when there is something; a link whose target is
    // gone is described by itself, and its access can't be changed.
    let (meta, target_found) = match fs::metadata(&path) {
        Ok(target) => (target, true),
        Err(_) => (own.clone(), false),
    };
    if meta.is_file()
        && let Ok(sniffed) = mime.type_of(&path)
    {
        entry.mime = sniffed;
    }
    let drive = is_mount_point(&path, &own).then(|| drive_space(&path)).flatten();
    Ok(FileProperties {
        folder: nova_protocol::wire_text(folder),
        entry,
        link_target: link_target.map(nova_protocol::wire_text),
        disk_bytes: (!meta.is_dir()).then(|| disk_bytes(&meta)),
        created_ms: meta.created().ok().and_then(millis),
        accessed_ms: meta.accessed().ok().and_then(millis),
        owner: crate::users::user_name(meta.uid()).unwrap_or_else(|| meta.uid().to_string()),
        group: crate::users::group_name(meta.gid()).unwrap_or_else(|| meta.gid().to_string()),
        access: access(meta.mode()),
        yours: target_found && meta.uid() == crate::users::current_uid(),
        drive,
    })
}

/// Lets the people `access` names read, change and run the item at `path`
/// (wire text), keeping its other mode bits.
pub fn set_access(path: &str, access: Access) -> Result<(), String> {
    let path = checked_path(path)?;
    let meta = fs::metadata(&path).map_err(|e| format!("Can't read {}: {e}", path.display()))?;
    if meta.uid() != crate::users::current_uid() {
        return Err(format!("Only its owner can change who may open {}", path.display()));
    }
    let mode = (meta.mode() & !ACCESS_BITS) | mode_bits(access);
    fs::set_permissions(&path, fs::Permissions::from_mode(mode))
        .map_err(|e| format!("Can't change who may open {}: {e}", path.display()))
}

fn access(mode: u32) -> Access {
    let rights = |shift: u32| {
        let bits = (mode >> shift) & ACCESS_BITS;
        Rights { read: bits & READ_BIT != 0, write: bits & WRITE_BIT != 0, run: bits & RUN_BIT != 0 }
    };
    Access { owner: rights(OWNER_SHIFT), group: rights(GROUP_SHIFT), others: rights(OTHERS_SHIFT) }
}

fn mode_bits(access: Access) -> u32 {
    let bits = |rights: Rights, shift: u32| {
        ((u32::from(rights.read) * READ_BIT)
            | (u32::from(rights.write) * WRITE_BIT)
            | (u32::from(rights.run) * RUN_BIT))
            << shift
    };
    bits(access.owner, OWNER_SHIFT) | bits(access.group, GROUP_SHIFT) | bits(access.others, OTHERS_SHIFT)
}

/// A drive is mounted at `path` (`own` is its own metadata, a link's not
/// followed): it's the root, or a folder on another device than the folder
/// it's in. A link to a folder on another drive is no mount.
fn is_mount_point(path: &Path, own: &Metadata) -> bool {
    if !own.is_dir() {
        return false;
    }
    match path.parent() {
        None => true,
        Some(parent) => fs::metadata(parent).is_ok_and(|parent| parent.dev() != own.dev()),
    }
}

fn drive_space(path: &Path) -> Option<DriveSpace> {
    let (size, used) = super::udisks::usage(&nova_protocol::wire_text(path))?;
    Some(DriveSpace { size, used })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    fn mime() -> MimeDb {
        MimeDb::load(&[]).0
    }

    fn wire(path: &Path) -> String {
        nova_protocol::wire_text(path)
    }

    #[test]
    fn a_file_reads_its_folder_size_times_owner_and_access() {
        let dir = TempDir::new("properties-file");
        let file = dir.join("notes.txt");
        fs::write(&file, b"hello there\n").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
        let read = read(&wire(&file), &mime()).unwrap();
        assert_eq!(read.folder, wire(&dir));
        assert_eq!(read.entry.name, "notes.txt");
        assert_eq!(read.entry.size, Some(12));
        assert!(read.entry.mime.starts_with("text/"), "sniffed as text: {}", read.entry.mime);
        assert!(read.disk_bytes.is_some());
        assert!(read.accessed_ms.is_some() && read.entry.modified_ms.is_some());
        assert!(read.yours);
        assert!(!read.owner.is_empty() && !read.group.is_empty());
        assert_eq!(
            read.access,
            Access {
                owner: Rights { read: true, write: true, run: false },
                group: Rights { read: true, write: false, run: false },
                others: Rights::default(),
            }
        );
        assert_eq!(read.drive, None, "a file is no drive");
    }

    #[test]
    fn a_link_says_where_it_points() {
        let dir = TempDir::new("properties-link");
        fs::create_dir(dir.join("Photos")).unwrap();
        std::os::unix::fs::symlink("Photos", dir.join("Pictures")).unwrap();
        let read = read(&wire(&dir.join("Pictures")), &mime()).unwrap();
        assert!(read.entry.link && read.entry.dir);
        assert_eq!(read.link_target.as_deref(), Some("Photos"));
        assert_eq!(read.disk_bytes, None, "a folder's size is measured");
    }

    #[test]
    fn a_broken_link_cant_have_its_access_changed() {
        let dir = TempDir::new("properties-broken");
        std::os::unix::fs::symlink("/nowhere", dir.join("broken")).unwrap();
        let read = read(&wire(&dir.join("broken")), &mime()).unwrap();
        assert_eq!(read.link_target.as_deref(), Some("/nowhere"));
        assert!(!read.yours);
        assert_eq!(read.drive, None);
    }

    #[test]
    fn the_root_is_a_drive_in_no_folder() {
        let read = read("/", &mime()).unwrap();
        assert_eq!(read.folder, "");
        assert!(read.entry.dir);
        let drive = read.drive.expect("the root is mounted");
        assert!(drive.size > 0 && drive.used <= drive.size);
    }

    #[test]
    fn a_missing_item_or_a_relative_path_is_refused() {
        let dir = TempDir::new("properties-missing");
        assert!(read(&wire(&dir.join("gone")), &mime()).unwrap_err().contains("doesn't exist"));
        assert!(read("notes.txt", &mime()).is_err());
    }

    #[test]
    fn setting_access_keeps_the_other_bits() {
        let dir = TempDir::new("properties-access");
        // The sticky bit stands for the bits access doesn't cover; setuid
        // can't be set inside Nix's build sandbox.
        let shared = dir.join("Shared");
        fs::create_dir(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o1700)).unwrap();
        let everyone = Rights { read: true, write: false, run: true };
        let access = Access { owner: Rights { read: true, write: true, run: true }, group: everyone, others: everyone };
        set_access(&wire(&shared), access).unwrap();
        assert_eq!(fs::metadata(&shared).unwrap().mode() & 0o7777, 0o1755);
        assert_eq!(read(&wire(&shared), &mime()).unwrap().access, access);
    }
}
