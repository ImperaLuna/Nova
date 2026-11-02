//! Where the search looks: the home folder and the drives mounted where
//! drives go (`/run/media/<user>`, as udisks2 mounts them, `/media` and
//! `/mnt`), from `/proc/self/mountinfo`.
//!
//! Inside them, a folder another filesystem is mounted on is followed only
//! when that filesystem is on a disk (its source is a `/dev/` node), so a
//! second disk mounted in Home is searched, but network shares, FUSE
//! mounts of phones or cloud drives, and virtual filesystems aren't: they
//! can be slow, huge or not real files.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// Where drives are mounted.
pub const DRIVE_PLACES: [&str; 3] = ["/run/media", "/media", "/mnt"];
/// The start of a mount source that is a disk.
const DEVICE_SOURCE: &[u8] = b"/dev/";

/// The mounts as the search sees them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mounts {
    /// Each mount point, and whether it's on a disk.
    points: HashMap<PathBuf, bool>,
}

impl Mounts {
    /// Read from `mountinfo`, the text of `/proc/self/mountinfo`.
    pub fn parse(mountinfo: &[u8]) -> Self {
        let points = mountinfo.split(|byte| *byte == b'\n').filter_map(mount).collect();
        Self { points }
    }

    /// Whether the walk goes into the folder `path`: it's no mount point,
    /// or one on a disk.
    pub fn follows(&self, path: &Path) -> bool {
        self.points.get(path).copied().unwrap_or(true)
    }

    /// The mount points inside `roots`, and whether each is on a disk:
    /// the mounts that change what's searched.
    pub fn inside(&self, roots: &[PathBuf]) -> Vec<(PathBuf, bool)> {
        let mut inside: Vec<(PathBuf, bool)> = self
            .points
            .iter()
            .filter(|(point, _)| roots.iter().any(|root| point.starts_with(root)))
            .map(|(point, on_disk)| (point.clone(), *on_disk))
            .collect();
        inside.sort();
        inside
    }

    /// The folders searched: `home`, then each drive on a disk mounted in
    /// one of `places`, by path, without any inside another.
    pub fn roots(&self, home: Option<&Path>, places: &[&Path]) -> Vec<PathBuf> {
        let mut drives: Vec<&PathBuf> = self
            .points
            .iter()
            .filter(|(point, on_disk)| **on_disk && places.iter().any(|place| point.starts_with(place)))
            .map(|(point, _)| point)
            .collect();
        drives.sort();
        let mut roots: Vec<PathBuf> = home.map(Path::to_path_buf).into_iter().collect();
        for drive in drives {
            if !roots.iter().any(|root| drive.starts_with(root)) {
                roots.push(drive.clone());
            }
        }
        roots
    }
}

/// One mountinfo line's mount point, and whether it's on a disk:
/// `id parent major:minor root point options [optional...] - type source
/// super-options`.
fn mount(line: &[u8]) -> Option<(PathBuf, bool)> {
    let mut fields = line.split(|byte| *byte == b' ');
    let point = fields.nth(4)?;
    let source = fields.skip_while(|field| *field != b"-").nth(2)?;
    let point = PathBuf::from(OsStr::from_bytes(&unescape(point)));
    Some((point, unescape(source).starts_with(DEVICE_SOURCE)))
}

/// A mountinfo field with its `\040`-style octal escapes (spaces, tabs,
/// newlines and backslashes) undone.
fn unescape(field: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(field.len());
    let mut at = 0;
    while at < field.len() {
        let octal = field.get(at + 1..at + 4).filter(|digits| digits.iter().all(|digit| (b'0'..=b'7').contains(digit)));
        match (field[at], octal) {
            (b'\\', Some(digits)) => {
                let value = digits.iter().fold(0u32, |value, digit| value * 8 + u32::from(digit - b'0'));
                out.push(u8::try_from(value).unwrap_or(b'?'));
                at += 4;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A laptop's mounts (shortened from a real one): the root and home on
    /// disks, a USB stick and a phone, a second disk in Home, a network
    /// share, and virtual filesystems.
    const MOUNTINFO: &str = "\
22 1 0:21 / /proc rw,nosuid shared:5 - proc proc rw
24 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw
30 24 259:3 / /home rw,relatime shared:2 - btrfs /dev/nvme0n1p3 rw
41 30 8:17 / /home/me/Games rw,relatime shared:9 - ext4 /dev/sdb1 rw
45 24 0:44 / /run/user/1000 rw shared:12 - tmpfs tmpfs rw
52 45 0:52 / /run/user/1000/gvfs rw shared:13 - fuse.gvfsd-fuse gvfsd-fuse rw
60 24 8:33 / /run/media/me/SANDISK\\040USB rw shared:20 - vfat /dev/sdc1 rw
61 24 0:60 / /run/media/me/Pixel rw shared:21 - fuse.jmtpfs jmtpfs rw
70 24 0:70 / /mnt/nas rw shared:30 - nfs4 nas:/share rw
71 24 8:49 / /mnt/backup rw shared:31 - ext4 /dev/sdd1 rw
72 71 8:50 / /mnt/backup/inner rw shared:32 - ext4 /dev/sdd2 rw
";

    fn places() -> Vec<&'static Path> {
        DRIVE_PLACES.iter().map(Path::new).collect()
    }

    #[test]
    fn home_comes_first_then_drives_on_disks() {
        let mounts = Mounts::parse(MOUNTINFO.as_bytes());
        assert_eq!(
            mounts.roots(Some(Path::new("/home/me")), &places()),
            [PathBuf::from("/home/me"), "/mnt/backup".into(), "/run/media/me/SANDISK USB".into()]
        );
        assert_eq!(mounts.roots(None, &places()).len(), 2);
    }

    #[test]
    fn the_walk_follows_disks_and_skips_other_mounts() {
        let mounts = Mounts::parse(MOUNTINFO.as_bytes());
        assert!(mounts.follows(Path::new("/home/me/Games")));
        assert!(mounts.follows(Path::new("/home/me/Documents")));
        assert!(!mounts.follows(Path::new("/run/user/1000/gvfs")));
        assert!(!mounts.follows(Path::new("/run/media/me/Pixel")));
        assert!(!mounts.follows(Path::new("/mnt/nas")));
    }

    #[test]
    fn only_mounts_inside_the_roots_change_what_is_searched() {
        let mounts = Mounts::parse(MOUNTINFO.as_bytes());
        let roots = mounts.roots(Some(Path::new("/home/me")), &places());
        assert_eq!(
            mounts.inside(&roots),
            [
                (PathBuf::from("/home/me/Games"), true),
                ("/mnt/backup".into(), true),
                ("/mnt/backup/inner".into(), true),
                ("/run/media/me/SANDISK USB".into(), true),
            ]
        );
    }

    #[test]
    fn broken_lines_are_skipped() {
        let mounts = Mounts::parse(b"garbage\n\n24 1 259:2 / / rw - ext4\n");
        assert_eq!(mounts, Mounts::default());
    }
}
