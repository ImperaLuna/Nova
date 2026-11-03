//! Archives in Nova Files (IMP-323): zips and tars (plain, gzip, zstd or
//! xz) open like read-only folders, extract, and selections pack into new
//! zips. Pure Rust throughout, so the static musl build keeps working.
//!
//! An archive is known by its name alone ([`Kind::of_name`]), like the
//! MIME types in a listing. A path that goes through one, such as
//! `~/photos.zip/2025`, names a folder inside it ([`Inside::of`]); views
//! show those (`views.rs`), and files opened there are first extracted on
//! their own ([`open_copy`]).
//!
//! - `read.rs`: the members of an archive, read once each time it's
//!   listed or extracted, with every name checked to stay inside it.
//! - `unpack.rs`: extracting all of them under one folder, which the copy
//!   engine (`transfers.rs`) then places as it does a move, asking about
//!   name clashes.
//! - `pack.rs`: writing a zip of files and folders.

mod pack;
mod read;
mod unpack;

pub use pack::pack;
pub use read::{MemberKind, members};
pub use unpack::{fresh_folder, total, unpack};

use crate::mime::MimeDb;
use nova_protocol::FileEntry;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

/// How an archive is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Zip,
    Tar,
    TarGz,
    TarZst,
    TarXz,
}

/// The name endings of each kind, the longer first so `.tar.gz` isn't
/// taken for something else.
const ENDINGS: &[(&str, Kind)] = &[
    (".tar.gz", Kind::TarGz),
    (".tar.zst", Kind::TarZst),
    (".tar.xz", Kind::TarXz),
    (".tgz", Kind::TarGz),
    (".tzst", Kind::TarZst),
    (".txz", Kind::TarXz),
    (".tar", Kind::Tar),
    (".zip", Kind::Zip),
];

impl Kind {
    /// The kind of archive a file named `name` is, case aside; none for
    /// anything else, or a name that is only the ending.
    pub fn of_name(name: &OsStr) -> Option<Self> {
        ending(name).map(|(_, kind)| kind)
    }
}

/// `name`'s archive ending and kind.
fn ending(name: &OsStr) -> Option<(usize, Kind)> {
    let name = name.as_bytes();
    ENDINGS.iter().find_map(|(ending, kind)| {
        let at = name.len().checked_sub(ending.len()).filter(|at| *at > 0)?;
        name[at..].eq_ignore_ascii_case(ending.as_bytes()).then_some((at, *kind))
    })
}

/// The name of the folder "Extract all" makes for the archive `name`:
/// `name` without its ending ("photos.tar.gz" gives "photos").
pub fn folder_name(name: &OsStr) -> OsString {
    let cut = ending(name).map_or(name.len(), |(at, _)| at);
    OsStr::from_bytes(&name.as_bytes()[..cut]).to_owned()
}

/// A folder inside an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inside {
    /// The archive file.
    pub archive: PathBuf,
    pub kind: Kind,
    /// The folder in it, relative; empty for its top.
    pub inner: PathBuf,
}

impl Inside {
    /// Where `path` goes into an archive: the first part of it named like
    /// one that is a file. Only those parts are looked at on disk, so a
    /// path through no archive name costs nothing, and a folder named
    /// "x.zip" stays a folder. Archives inside archives aren't opened.
    pub fn of(path: &Path) -> Option<Self> {
        let mut archive = PathBuf::new();
        let mut parts = path.components();
        for part in parts.by_ref() {
            archive.push(part);
            let Component::Normal(name) = part else { continue };
            let Some(kind) = Kind::of_name(name) else { continue };
            if std::fs::metadata(&archive).is_ok_and(|meta| meta.is_file()) {
                return Some(Self { archive, kind, inner: parts.collect() });
            }
        }
        None
    }
}

/// The entries of the folder `inside` shows, hidden ones only when
/// `hidden`, in no order. Folders an archive only implies (it lists
/// "a/b.txt" but not "a/") are listed too. `None` once `stopped`.
pub fn list(
    inside: &Inside,
    hidden: bool,
    mime: &MimeDb,
    stopped: &dyn Fn() -> bool,
) -> io::Result<Option<Vec<FileEntry>>> {
    let Some(members) = members(&inside.archive, inside.kind, stopped)? else { return Ok(None) };
    let mut found = inside.inner.as_os_str().is_empty();
    let mut entries: BTreeMap<OsString, FileEntry> = BTreeMap::new();
    for member in &members {
        let Ok(rest) = member.path.strip_prefix(&inside.inner) else { continue };
        let mut below = rest.components();
        let Some(Component::Normal(name)) = below.next() else {
            // The folder itself.
            found |= member.kind == MemberKind::Dir;
            continue;
        };
        found = true;
        let implied = below.next().is_some();
        let wire = nova_protocol::wire_text(name);
        let is_hidden = crate::files::folder::is_hidden(&wire);
        if (!hidden && is_hidden) || (implied && entries.contains_key(name)) {
            continue;
        }
        // A member listed after what's inside it keeps being a folder.
        let had_children = entries.get(name).is_some_and(|known| known.dir);
        let dir = implied || had_children || member.kind == MemberKind::Dir;
        let mime = if dir { crate::mime::DIRECTORY } else { mime.type_of_name(&name.to_string_lossy(), member.size) };
        let entry = FileEntry {
            mime: mime.to_owned(),
            archive: false,
            name: wire,
            dir,
            link: !implied && member.kind == MemberKind::Link,
            hidden: is_hidden,
            size: (!dir).then_some(member.size),
            modified_ms: if implied { None } else { member.modified_ms },
        };
        entries.insert(name.to_owned(), entry);
    }
    if !found {
        return Err(io::Error::new(io::ErrorKind::NotFound, "not in the archive"));
    }
    Ok((!stopped()).then(|| entries.into_values().collect()))
}

/// Extracts the file `inside` names (its folder is the archive and the
/// path in it) into a new folder under `copies`, read-only, for an app
/// to open; answers its path. Each copy gets its own folder, so two files
/// of the same name don't meet.
pub fn open_copy(inside: &Inside, copies: &Path) -> io::Result<PathBuf> {
    std::fs::create_dir_all(copies)?;
    let folder = fresh_folder(copies, OsStr::new("open"))?;
    let copied = unpack::unpack_one(inside, &folder);
    if copied.is_err() {
        let _ = std::fs::remove_dir_all(&folder);
    }
    copied
}

#[cfg(test)]
pub(crate) mod tests;
