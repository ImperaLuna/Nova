//! Extracting an archive's members under one folder.
//!
//! Files and folders are written first, in the archive's order, then
//! hard links (as copies of the file they repeat), then symbolic links,
//! then each folder's permissions and time. No link exists while anything
//! is written, so a member can't be written through one to somewhere
//! else. Permissions keep what the archive says minus write for others
//! (and never setuid or setgid); files are private while they fill.

use super::read::{self, Member, MemberKind};
use super::{Inside, Kind};
use std::cell::Cell;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

/// Bytes copied between checks for a stop.
const CHUNK_BYTES: usize = 1 << 20;
/// Permission bits an extracted file or folder can keep: no setuid,
/// setgid, sticky or write for others.
const KEPT_MODE: u32 = 0o755;
/// What a member without permissions gets.
const FILE_MODE: u32 = 0o644;
const DIR_MODE: u32 = 0o755;
/// An opened copy can only be read: changes to it wouldn't reach the
/// archive.
const OPENED_MODE: u32 = 0o444;
/// Tries at a free hidden folder name.
const MOST_TRIES: u32 = 10_000;

/// What extracting is measured against, for its progress: see
/// [`read::total`].
pub fn total(archive: &Path, kind: Kind) -> io::Result<u64> {
    read::total(archive, kind)
}

/// Extracts the archive `archive` into the folder `into`, which is empty.
/// `progress` hears how much of [`total`] is done after each chunk, and
/// answers false to stop; then this answers false, leaving what it wrote.
pub fn unpack(archive: &Path, kind: Kind, into: &Path, progress: &mut dyn FnMut(u64) -> bool) -> io::Result<bool> {
    let done = Cell::new(0);
    let mut later = Later::default();
    let finished = read::each(archive, kind, &done, &mut |member, contents| {
        let target = into.join(&member.path);
        match member.kind {
            MemberKind::File => {
                if let Some(parent) = target.parent() {
                    fs::DirBuilder::new().recursive(true).mode(DIR_MODE).create(parent)?;
                }
                if !write_file(&member, contents, &target, &mut || progress(done.get()))? {
                    return Ok(false);
                }
            }
            MemberKind::Dir => {
                fs::DirBuilder::new().recursive(true).mode(DIR_MODE).create(&target)?;
                later.folders.push((target, member));
            }
            MemberKind::Link | MemberKind::HardLink => later.links.push((target, member)),
            MemberKind::Other => {}
        }
        Ok(progress(done.get()))
    })?;
    if finished {
        later.finish(into)?;
    }
    Ok(finished)
}

/// Extracts the one file `inside` names into the folder `into`, readable
/// only, and answers its path.
pub fn unpack_one(inside: &Inside, into: &Path) -> io::Result<PathBuf> {
    let name = inside.inner.file_name().ok_or_else(|| io::Error::from(io::ErrorKind::IsADirectory))?;
    let target = into.join(name);
    let mut wrote = false;
    read::each(&inside.archive, inside.kind, &Cell::new(0), &mut |member, contents| {
        if member.path != inside.inner {
            return Ok(true);
        }
        if member.kind != MemberKind::File {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "only files open from an archive"));
        }
        // A later member of the same name replaces it, as extracting does.
        wrote = write_file(&Member { mode: Some(OPENED_MODE), ..member }, contents, &target, &mut || true)?;
        Ok(true)
    })?;
    if wrote { Ok(target) } else { Err(io::Error::new(io::ErrorKind::NotFound, "not in the archive")) }
}

/// A new private folder named `.nova-<what>.<number>` in `parent`.
pub fn fresh_folder(parent: &Path, what: &OsStr) -> io::Result<PathBuf> {
    for attempt in 0..MOST_TRIES {
        let mut name = OsStr::new(".nova-").to_owned();
        name.push(what);
        name.push(format!(".{attempt}"));
        let folder = parent.join(name);
        match fs::DirBuilder::new().mode(0o700).create(&folder) {
            Ok(()) => return Ok(folder),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no free name for a folder to extract into"))
}

/// Writes `member`'s `contents` to `target`, replacing a file there;
/// false when `go_on` stops it between chunks.
fn write_file(member: &Member, contents: &mut dyn Read, target: &Path, go_on: &mut dyn FnMut() -> bool) -> io::Result<bool> {
    let mut file = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(target)?;
    let mut buffer = vec![0; CHUNK_BYTES];
    loop {
        let read = match contents.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        file.write_all(&buffer[..read])?;
        if !go_on() {
            return Ok(false);
        }
    }
    settle(&file, member, FILE_MODE)?;
    Ok(true)
}

/// Gives `file` the permissions and time `member` has.
fn settle(file: &File, member: &Member, default_mode: u32) -> io::Result<()> {
    let mode = member.mode.map_or(default_mode, |mode| mode & KEPT_MODE);
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    if let Some(time) = member.modified_ms.and_then(|ms| u64::try_from(ms).ok()) {
        file.set_modified(UNIX_EPOCH + Duration::from_millis(time))?;
    }
    Ok(())
}

/// What's done once every file is in.
#[derive(Default)]
struct Later {
    folders: Vec<(PathBuf, Member)>,
    links: Vec<(PathBuf, Member)>,
}

impl Later {
    fn finish(self, into: &Path) -> io::Result<()> {
        // Hard links first: one may repeat a file a symbolic link would
        // stand in the way of.
        let (hard, soft): (Vec<_>, Vec<_>) =
            self.links.into_iter().partition(|(_, member)| member.kind == MemberKind::HardLink);
        for (target, member) in hard {
            let Some(original) = member.target.as_ref().map(|original| into.join(original)) else { continue };
            if fs::symlink_metadata(&original).is_ok_and(|meta| meta.is_file()) && original != target {
                make_parent(&target)?;
                fs::copy(&original, &target)?;
            }
        }
        for (target, member) in soft {
            let Some(points_to) = &member.target else { continue };
            // Never made through another link ("a" to /etc, then "a/x").
            if !through_folders(into, &target) {
                continue;
            }
            make_parent(&target)?;
            match std::os::unix::fs::symlink(points_to, &target) {
                Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
                _ => {}
            }
        }
        // Deepest first, so setting a folder's time comes after its
        // entries are all in.
        for (target, member) in self.folders.into_iter().rev() {
            let folder = File::open(&target)?;
            // Its owner can always go in and write.
            let member = Member { mode: Some(member.mode.unwrap_or(DIR_MODE) | 0o700), ..member };
            settle(&folder, &member, DIR_MODE)?;
        }
        Ok(())
    }
}

/// Whether nothing between `into` and `path` is a link.
fn through_folders(into: &Path, path: &Path) -> bool {
    path.ancestors()
        .skip(1)
        .take_while(|above| *above != into)
        .all(|above| fs::symlink_metadata(above).map_or(true, |meta| !meta.file_type().is_symlink()))
}

/// Makes the folder `path` goes in. Links are made after every file, so
/// only folders the archive made are on the way.
fn make_parent(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) => fs::DirBuilder::new().recursive(true).mode(DIR_MODE).create(parent),
        None => Ok(()),
    }
}
