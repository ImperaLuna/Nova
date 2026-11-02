//! The file system side of copying and moving ([`super::transfers`]):
//! measuring what a job will copy, the clashes it will meet, a free
//! numbered name, and copying one file in chunks so a job can report
//! progress and stop halfway.
//!
//! Nothing here follows symbolic links: a link is copied as a link, and a
//! folder is walked by its own entries.

use nova_protocol::ClashItem;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// How much of a file is copied between progress reports and checks for a
/// stop.
pub const CHUNK_BYTES: usize = 1 << 20;
/// The highest number tried for a free name ("cover (2).png" and on).
const MOST_NUMBERED: u32 = 10_000;
/// Ends the name of a file being copied, until it's complete and renamed
/// to its own name.
const PART_SUFFIX: &str = ".nova-part";

/// What a job will copy or move, and the clashes it will meet on the way
/// if every folder clash is merged.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Measure {
    pub bytes: u64,
    pub file_clashes: u32,
    pub folder_clashes: u32,
}

impl Measure {
    pub fn add(&mut self, other: Self) {
        self.bytes += other.bytes;
        self.file_clashes += other.file_clashes;
        self.folder_clashes += other.folder_clashes;
    }
}

/// Whether two entries clash as folders (a folder landing on a folder,
/// which can merge) rather than as files.
pub fn folders(source: &Metadata, existing: &Metadata) -> bool {
    source.is_dir() && existing.is_dir()
}

/// Measures `source` as it would land at `target`: its bytes, and the
/// clashes with what's at `target` and, for folders that would merge,
/// inside it. `None` when `stopped` says to stop.
pub fn measure(source: &Path, target: Option<&Path>, stopped: &dyn Fn() -> bool) -> io::Result<Option<Measure>> {
    if stopped() {
        return Ok(None);
    }
    let meta = fs::symlink_metadata(source)?;
    let mut measured = Measure::default();
    let existing = target.and_then(|target| fs::symlink_metadata(target).ok());
    let mut merges = false;
    if let Some(existing) = &existing {
        if folders(&meta, existing) {
            measured.folder_clashes += 1;
            merges = true;
        } else {
            measured.file_clashes += 1;
        }
    }
    if meta.is_file() {
        measured.bytes = meta.len();
    } else if meta.is_dir() {
        for entry in fs::read_dir(source)? {
            let name = entry?.file_name();
            let inner = target.filter(|_| merges).map(|target| target.join(&name));
            let Some(child) = measure(&source.join(&name), inner.as_deref(), stopped)? else { return Ok(None) };
            measured.add(child);
        }
    }
    Ok(Some(measured))
}

/// One side of a clash, as the dialog shows it.
pub fn clash_item(path: &Path, meta: &Metadata) -> ClashItem {
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|since| i64::try_from(since.as_millis()).ok());
    if meta.is_dir() {
        let items = fs::read_dir(path).map(|entries| entries.count()).unwrap_or(0);
        ClashItem { size: None, items: Some(u32::try_from(items).unwrap_or(u32::MAX)), modified_ms }
    } else {
        ClashItem { size: Some(meta.len()), items: None, modified_ms }
    }
}

/// A name for a copy of `name` that's free in `folder`: "cover (2).png",
/// "cover (3).png", ..., counting on from a number the name already has.
/// A folder's name, or a file's without an extension, gets the number at
/// the end.
pub fn numbered(folder: &Path, name: &OsStr, dir: bool) -> io::Result<OsString> {
    let bytes = name.as_bytes();
    // A leading dot starts a hidden name, not an extension.
    let split = (!dir).then(|| bytes.iter().rposition(|byte| *byte == b'.')).flatten().filter(|at| *at > 0);
    let (stem, extension) = split.map_or((bytes, &b""[..]), |at| bytes.split_at(at));
    let (base, first) = numbered_parts(stem);
    for number in first..=MOST_NUMBERED {
        let mut candidate = base.to_vec();
        candidate.extend_from_slice(format!(" ({number})").as_bytes());
        candidate.extend_from_slice(extension);
        let candidate = OsString::from(OsStr::from_bytes(&candidate));
        match fs::symlink_metadata(folder.join(&candidate)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Err(e) => return Err(e),
            Ok(_) => {}
        }
    }
    Err(io::Error::other(format!("every name up to ({MOST_NUMBERED}) is taken")))
}

/// A stem without its " (n)", and the number to try first.
fn numbered_parts(stem: &[u8]) -> (&[u8], u32) {
    let number = stem
        .strip_suffix(b")")
        .and_then(|rest| rest.iter().rposition(|byte| *byte == b'(').map(|at| (&rest[..at], &rest[at + 1..])))
        .and_then(|(before, digits)| Some((before.strip_suffix(b" ")?, std::str::from_utf8(digits).ok()?)))
        .filter(|(_, digits)| !digits.starts_with('0') && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|(before, digits)| Some((before, digits.parse::<u32>().ok()?)));
    match number {
        Some((before, number)) if !before.is_empty() => (before, number.saturating_add(1).max(2)),
        _ => (stem, 2),
    }
}

/// A file being written next to where it will land, removed unless it's
/// [`Part::finish`]ed: a copy that stops halfway leaves nothing behind.
pub struct Part {
    path: PathBuf,
    file: Option<File>,
}

impl Part {
    /// Creates it in `folder` for an item called `name`, private until it's
    /// finished.
    pub fn create(folder: &Path, name: &OsStr) -> io::Result<Self> {
        let mut base = OsString::from(".");
        base.push(name);
        for attempt in 0..MOST_NUMBERED {
            let mut candidate = base.clone();
            if attempt > 0 {
                candidate.push(format!(".{attempt}"));
            }
            candidate.push(PART_SUFFIX);
            let path = folder.join(candidate);
            match OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path) {
                Ok(file) => return Ok(Self { path, file: Some(file) }),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other("no free name for a partial copy"))
    }

    /// Writes `source`'s content into it, a chunk at a time, calling
    /// `copied` with each chunk's size. False when `copied` says to stop.
    pub fn fill(&mut self, source: &Path, copied: &mut dyn FnMut(u64) -> bool) -> io::Result<bool> {
        let Some(file) = self.file.as_mut() else { return Ok(false) };
        let mut input = File::open(source)?;
        let mut buffer = vec![0; CHUNK_BYTES];
        loop {
            let read = match input.read(&mut buffer) {
                Ok(0) => return Ok(true),
                Ok(read) => read,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            file.write_all(&buffer[..read])?;
            if !copied(read as u64) {
                return Ok(false);
            }
        }
    }

    /// Gives it `meta`'s permissions and modified time, flushes it and puts
    /// it at `target`, over anything there.
    pub fn finish(mut self, meta: &Metadata, target: &Path) -> io::Result<()> {
        let Some(file) = self.file.take() else { return Err(io::Error::other("already finished")) };
        if let Ok(modified) = meta.modified() {
            file.set_modified(modified)?;
        }
        file.set_permissions(meta.permissions())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&self.path, target)
    }
}

impl Drop for Part {
    fn drop(&mut self) {
        if self.file.take().is_some() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Copies the symbolic link `source` to `target`, over anything there that
/// isn't a folder.
pub fn copy_link(source: &Path, target: &Path) -> io::Result<()> {
    let points_to = fs::read_link(source)?;
    let folder = target.parent().unwrap_or(Path::new("/"));
    let name = target.file_name().unwrap_or_default();
    // Made under a temporary name and renamed, so what was there is
    // replaced in one step.
    let part = Part::create(folder, name)?;
    let temporary = part.path.clone();
    drop(part);
    std::os::unix::fs::symlink(points_to, &temporary)?;
    fs::rename(&temporary, target).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

/// Removes whatever is at `path`, a whole folder included.
pub fn remove(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    fn name(folder: &Path, of: &str, dir: bool) -> String {
        numbered(folder, OsStr::new(of), dir).unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn a_copy_gets_the_first_free_number_before_the_extension() {
        let dir = TempDir::new("numbered");
        fs::write(dir.join("cover.png"), "").unwrap();
        assert_eq!(name(&dir, "cover.png", false), "cover (2).png");
        fs::write(dir.join("cover (2).png"), "").unwrap();
        assert_eq!(name(&dir, "cover.png", false), "cover (3).png");
        assert_eq!(name(&dir, "cover (2).png", false), "cover (3).png", "counts on from its own number");
        assert_eq!(name(&dir, "Photos 2025", true), "Photos 2025 (2)");
        assert_eq!(name(&dir, "v1.2", true), "v1.2 (2)", "a folder has no extension");
        assert_eq!(name(&dir, ".bashrc", false), ".bashrc (2)");
        assert_eq!(name(&dir, "notes (draft).txt", false), "notes (draft) (2).txt");
        assert_eq!(name(&dir, "(1)", false), "(1) (2)");
    }

    #[test]
    fn measuring_counts_bytes_and_the_clashes_a_merge_meets() {
        let dir = TempDir::new("measure");
        let (from, to) = (dir.join("from/Photos"), dir.join("to/Photos"));
        fs::create_dir_all(from.join("2025")).unwrap();
        fs::create_dir_all(to.join("2025")).unwrap();
        fs::write(from.join("a.png"), "12345").unwrap();
        fs::write(from.join("2025/b.png"), "123").unwrap();
        fs::write(to.join("2025/b.png"), "old").unwrap();
        let measured = measure(&from, Some(&to), &|| false).unwrap().unwrap();
        assert_eq!(measured, Measure { bytes: 8, file_clashes: 1, folder_clashes: 2 });
        let alone = measure(&from, None, &|| false).unwrap().unwrap();
        assert_eq!(alone, Measure { bytes: 8, file_clashes: 0, folder_clashes: 0 });
        assert_eq!(measure(&from, None, &|| true).unwrap(), None);
    }

    #[test]
    fn a_part_left_unfinished_is_removed() {
        let dir = TempDir::new("part");
        fs::write(dir.join("big.bin"), vec![7; CHUNK_BYTES + 10]).unwrap();
        let mut part = Part::create(&dir, OsStr::new("big.bin")).unwrap();
        let mut chunks = 0;
        assert!(
            !part
                .fill(&dir.join("big.bin"), &mut |_| {
                    chunks += 1;
                    false
                })
                .unwrap()
        );
        drop(part);
        assert_eq!(chunks, 1);
        let left: Vec<_> = fs::read_dir(&*dir).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(left, [OsString::from("big.bin")]);
    }
}
