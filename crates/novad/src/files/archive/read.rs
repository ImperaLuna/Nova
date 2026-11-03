//! Reading an archive's members, in the order it stores them.
//!
//! A zip is read from its table of contents ([`members`] doesn't
//! decompress anything); a tar is read from start to end, through its
//! decompressor. Every member's path is checked to stay inside the
//! archive ([`enclosed`]): one starting at `/` or going up with `..` is
//! left out, so nothing can reach outside the folder it's extracted to.

use super::Kind;
use std::cell::Cell;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::os::unix::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf};

/// The longest link target read from a zip, like the kernel's `PATH_MAX`.
const MOST_LINK_BYTES: u64 = 4096;

/// One member of an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// Relative, with only plain names in it.
    pub path: PathBuf,
    pub kind: MemberKind,
    /// A file's size once extracted.
    pub size: u64,
    pub modified_ms: Option<i64>,
    /// Its Unix permissions, when the archive keeps them.
    pub mode: Option<u32>,
    /// What a link points to; for a hard link, the member it repeats
    /// (checked like a member's path). Read only while extracting.
    pub target: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    File,
    Dir,
    Link,
    HardLink,
    /// A device, a pipe: never extracted.
    Other,
}

/// Every member of the archive at `path`, without contents. `None` once
/// `stopped`, which is asked before each one.
pub fn members(path: &Path, kind: Kind, stopped: &dyn Fn() -> bool) -> io::Result<Option<Vec<Member>>> {
    let mut found = Vec::new();
    if kind == Kind::Zip {
        let mut zip = open_zip(path)?;
        for index in 0..zip.len() {
            if stopped() {
                return Ok(None);
            }
            let file = zip.by_index_raw(index).map_err(zip_error)?;
            found.extend(zip_member(&file));
        }
        return Ok(Some(found));
    }
    let counted = Cell::new(0);
    let finished = each(path, kind, &counted, &mut |member, _| {
        found.push(member);
        Ok(!stopped())
    })?;
    Ok(finished.then_some(found))
}

/// How much [`each`] counts in `done` by the end: a zip's bytes once
/// extracted, a tar's bytes on disk (decompressing one is the only way to
/// learn its size).
pub(super) fn total(path: &Path, kind: Kind) -> io::Result<u64> {
    if kind == Kind::Zip {
        let zip = open_zip(path)?;
        return Ok(zip.decompressed_size().map_or(0, |size| u64::try_from(size).unwrap_or(u64::MAX)));
    }
    Ok(std::fs::metadata(path)?.len())
}

/// Gives each member of the archive at `path` to `visit` with its
/// contents, and a link's target in it, until `visit` answers false;
/// answers whether it got through all of them. `done` counts what
/// [`total`] measures as it's read.
pub(super) fn each(
    path: &Path,
    kind: Kind,
    done: &Cell<u64>,
    visit: &mut dyn FnMut(Member, &mut dyn Read) -> io::Result<bool>,
) -> io::Result<bool> {
    if kind == Kind::Zip {
        let mut zip = open_zip(path)?;
        for index in 0..zip.len() {
            let mut file = zip.by_index(index).map_err(zip_error)?;
            let Some(mut member) = zip_member(&file) else { continue };
            if member.kind == MemberKind::Link {
                let mut target = Vec::new();
                (&mut file).take(MOST_LINK_BYTES).read_to_end(&mut target)?;
                member.target = Some(PathBuf::from(OsString::from_vec(target)));
            }
            if !visit(member, &mut Counting { inner: &mut file, done })? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    let file = Counting { inner: BufReader::new(File::open(path)?), done };
    let decompressed: Box<dyn Read + '_> = match kind {
        Kind::Tar => Box::new(file),
        Kind::TarGz => Box::new(flate2::read::MultiGzDecoder::new(file)),
        Kind::TarZst => Box::new(ruzstd::decoding::StreamingDecoder::new(file).map_err(io::Error::other)?),
        Kind::TarXz => Box::new(lzma_rust2::XzReader::new(file, true)),
        Kind::Zip => unreachable!("read above"),
    };
    let mut tar = tar::Archive::new(decompressed);
    for entry in tar.entries()? {
        let mut entry = entry?;
        let Some(member) = tar_member(&entry)? else { continue };
        if !visit(member, &mut entry)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn open_zip(path: &Path) -> io::Result<zip::ZipArchive<BufReader<File>>> {
    zip::ZipArchive::new(BufReader::new(File::open(path)?)).map_err(zip_error)
}

fn zip_member<R: Read>(file: &zip::read::ZipFile<'_, R>) -> Option<Member> {
    let path = enclosed(&file.enclosed_name()?)?;
    let kind = if file.is_dir() {
        MemberKind::Dir
    } else if file.is_symlink() {
        MemberKind::Link
    } else {
        MemberKind::File
    };
    let modified_ms = file.last_modified().and_then(|time| {
        let civil = jiff::civil::DateTime::try_from(time).ok()?;
        Some(civil.to_zoned(jiff::tz::TimeZone::system()).ok()?.timestamp().as_millisecond())
    });
    let size = if kind == MemberKind::File { file.size() } else { 0 };
    Some(Member { path, kind, size, modified_ms, mode: file.unix_mode(), target: None })
}

fn tar_member<R: Read>(entry: &tar::Entry<'_, R>) -> io::Result<Option<Member>> {
    let Some(path) = enclosed(&entry.path()?) else { return Ok(None) };
    let header = entry.header();
    let kind = header.entry_type();
    let (kind, target) = if kind.is_dir() {
        (MemberKind::Dir, None)
    } else if kind.is_symlink() {
        (MemberKind::Link, entry.link_name()?.map(|target| target.into_owned()))
    } else if kind.is_hard_link() {
        (MemberKind::HardLink, entry.link_name()?.and_then(|target| enclosed(&target)))
    } else if kind.is_file() || kind.is_contiguous() || kind.is_gnu_sparse() {
        (MemberKind::File, None)
    } else {
        (MemberKind::Other, None)
    };
    let size = if kind == MemberKind::File { entry.size() } else { 0 };
    let modified_ms = header.mtime().ok().and_then(|seconds| i64::try_from(seconds).ok()?.checked_mul(1000));
    Ok(Some(Member { path, kind, size, modified_ms, mode: header.mode().ok(), target }))
}

/// `path` with only its plain names, or `None` when it starts at `/` or
/// goes up, or names nothing.
pub(super) fn enclosed(path: &Path) -> Option<PathBuf> {
    let mut kept = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(name) => kept.push(name),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!kept.as_os_str().is_empty()).then_some(kept)
}

/// A zip's error as the window words it.
fn zip_error(error: zip::result::ZipError) -> io::Error {
    match error {
        zip::result::ZipError::Io(error) => error,
        zip::result::ZipError::UnsupportedArchive(why) => {
            io::Error::new(io::ErrorKind::Unsupported, format!("it uses what Nova can't read: {why}"))
        }
        zip::result::ZipError::CompressionMethodNotSupported(_) => {
            io::Error::new(io::ErrorKind::Unsupported, "it's compressed in a way Nova can't read")
        }
        zip::result::ZipError::InvalidPassword => {
            io::Error::new(io::ErrorKind::Unsupported, "it's protected with a password")
        }
        other => io::Error::new(io::ErrorKind::InvalidData, format!("it's damaged: {other}")),
    }
}

/// Counts what's read through it in `done`.
struct Counting<'a, R> {
    inner: R,
    done: &'a Cell<u64>,
}

impl<R: Read> Read for Counting<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.done.set(self.done.get().saturating_add(read as u64));
        Ok(read)
    }
}
