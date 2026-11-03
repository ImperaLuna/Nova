//! What type of file something is, from the freedesktop.org shared MIME
//! database (`mime/` in each XDG data folder, the files
//! `update-mime-database` writes): its name's patterns (`globs`), its
//! first bytes (`magic`), the old names of types (`aliases`) and the types
//! each is a kind of (`subclasses`).
//!
//! Two lookups: [`MimeDb::type_by_name`] only looks at a name and the
//! file's metadata, for listing big folders, and [`MimeDb::type_of`] also
//! reads the file's first bytes when the name doesn't settle it, the way
//! the spec orders them: the name first, contents for a name that matches
//! no pattern or several, then "text or binary". [`Mime`] keeps it up to
//! date as packages bring new types (`follow.rs`).

mod follow;
mod globs;
mod magic;

pub use follow::Mime;

use globs::Globs;
use magic::Magic;
use std::collections::HashMap;
use std::fs::Metadata;
use std::io::Read;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

/// A folder.
pub const DIRECTORY: &str = "inode/directory";
/// A file of nothing.
pub const EMPTY: &str = "application/x-zerosize";
/// Plain text, which every `text/*` type is a kind of.
pub const TEXT: &str = "text/plain";
/// Any bytes, which every type that isn't `inode/*` is a kind of.
pub const BINARY: &str = "application/octet-stream";
/// How much of a file with no type from its name or contents is checked
/// for being text.
const TEXT_SNIFF: usize = 512;
/// What a database file may weigh at most; the real ones are well under.
const MAX_DB_FILE: u64 = 16 * 1024 * 1024;

/// The database, read from every data folder.
#[derive(Debug, Default)]
pub struct MimeDb {
    globs: Globs,
    magic: Magic,
    /// Old names to the type's own.
    aliases: HashMap<String, String>,
    /// Each type's direct parents.
    parents: HashMap<String, Vec<String>>,
}

impl MimeDb {
    /// Reads `mime/` in each of `data_dirs` (the most important first) and
    /// the problems met, for the log. With none there it knows only folders
    /// and the like, text and binary.
    pub fn load(data_dirs: &[PathBuf]) -> (Self, Vec<String>) {
        let mut db = Self::default();
        let mut problems = Vec::new();
        // The least important first, so a later folder's say wins.
        for dir in data_dirs.iter().rev().map(|dir| dir.join("mime")) {
            if let Some(text) = read_text(&dir.join("globs2"), &mut problems) {
                db.globs.add(&text);
            }
            if let Some(bytes) = read(&dir.join("magic"), &mut problems)
                && let Err(e) = db.magic.add(&bytes)
            {
                problems.push(format!("{}: {e}", dir.join("magic").display()));
            }
            for (alias, canonical) in read_text(&dir.join("aliases"), &mut problems).iter().flat_map(|text| pairs(text))
            {
                db.aliases.insert(alias.to_string(), canonical.to_string());
            }
            for (child, parent) in read_text(&dir.join("subclasses"), &mut problems).iter().flat_map(|text| pairs(text))
            {
                let parents = db.parents.entry(child.to_string()).or_default();
                if !parents.iter().any(|known| known == parent) {
                    parents.push(parent.to_string());
                }
            }
        }
        (db, problems)
    }

    /// The type of the file `name` whose metadata is `metadata`, from its
    /// name alone: never opens the file. A name several types claim gets
    /// the first; one no type claims is binary.
    pub fn type_by_name<'a>(&'a self, name: &str, metadata: &Metadata) -> &'a str {
        if let Some(special) = special_type(metadata) {
            return special;
        }
        if metadata.len() == 0 {
            return EMPTY;
        }
        self.globs.find(name).types.first().copied().unwrap_or(BINARY)
    }

    /// The type of the file at `path`, reading its first bytes when its
    /// name matches no pattern or several. Follows links, like opening it.
    pub fn type_of(&self, path: &Path) -> std::io::Result<String> {
        let metadata = std::fs::metadata(path)?;
        if let Some(special) = special_type(&metadata) {
            return Ok(special.to_string());
        }
        let name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
        let by_name = self.globs.find(&name).types;
        if let [only] = by_name.as_slice() {
            return Ok(self.canonical(only).to_string());
        }
        if metadata.len() == 0 {
            return Ok(EMPTY.to_string());
        }
        let mut start = Vec::new();
        std::fs::File::open(path)?.take(self.magic.extent().max(TEXT_SNIFF) as u64).read_to_end(&mut start)?;
        // A tie between names is settled by contents among those types.
        let among = |mime: &str| by_name.iter().any(|named| self.is_a(mime, named) || self.is_a(named, mime));
        let only: Option<&dyn Fn(&str) -> bool> = if by_name.is_empty() { None } else { Some(&among) };
        let found = match self.magic.find(&start, only) {
            // A name's type that's a kind of what the contents say is the
            // more exact: a Word file's name over the contents' OLE storage.
            Some((mime, _)) => Some(by_name.iter().copied().find(|named| self.is_a(named, mime)).unwrap_or(mime)),
            None => by_name.first().copied(),
        };
        Ok(match found {
            Some(mime) => self.canonical(mime).to_string(),
            None if looks_like_text(&start) => TEXT.to_string(),
            None => BINARY.to_string(),
        })
    }

    /// The type's own name, for one known by an old one.
    pub fn canonical<'a>(&'a self, mime: &'a str) -> &'a str {
        self.aliases.get(mime).map_or(mime, String::as_str)
    }

    /// `mime` and every type it's a kind of, the nearest first: its
    /// parents in the database, then plain text when it or one of them is
    /// `text/*`, then binary for anything that isn't `inode/*`.
    pub fn with_parents(&self, mime: &str) -> Vec<String> {
        let mut found = vec![self.canonical(mime).to_string()];
        let mut next = 0;
        while next < found.len() {
            let current = found[next].clone();
            next += 1;
            for parent in self.parents.get(&current).into_iter().flatten() {
                let parent = self.canonical(parent).to_string();
                if !found.contains(&parent) {
                    found.push(parent);
                }
            }
        }
        let is_text = found.iter().any(|known| known.starts_with("text/"));
        let implicit = [(is_text, TEXT), (!found[0].starts_with("inode/"), BINARY)];
        for (applies, parent) in implicit {
            if applies && !found.iter().any(|known| known == parent) {
                found.push(parent.to_string());
            }
        }
        found
    }

    /// Whether `mime` is `base` or a kind of it.
    pub fn is_a(&self, mime: &str, base: &str) -> bool {
        self.with_parents(mime).iter().any(|known| known == self.canonical(base))
    }
}

/// The `inode/*` type of what isn't a plain file.
fn special_type(metadata: &Metadata) -> Option<&'static str> {
    let kind = metadata.file_type();
    if kind.is_dir() {
        Some(DIRECTORY)
    } else if kind.is_symlink() {
        Some("inode/symlink")
    } else if kind.is_block_device() {
        Some("inode/blockdevice")
    } else if kind.is_char_device() {
        Some("inode/chardevice")
    } else if kind.is_fifo() {
        Some("inode/fifo")
    } else if kind.is_socket() {
        Some("inode/socket")
    } else {
        None
    }
}

/// Text when it's UTF-8 (cut anywhere at the end) without control
/// characters other than whitespace.
fn looks_like_text(start: &[u8]) -> bool {
    let start = &start[..start.len().min(TEXT_SNIFF)];
    let valid = match std::str::from_utf8(start) {
        Ok(text) => text,
        // Only an incomplete character at the very end is forgiven.
        Err(e) if e.error_len().is_none() => std::str::from_utf8(&start[..e.valid_up_to()]).unwrap_or_default(),
        Err(_) => return false,
    };
    !valid.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\x0c'))
}

/// `a b` lines.
fn pairs(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines().filter(|line| !line.starts_with('#')).filter_map(|line| line.split_once(' '))
}

fn read(path: &Path, problems: &mut Vec<String>) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    match std::fs::File::open(path).and_then(|file| file.take(MAX_DB_FILE).read_to_end(&mut bytes)) {
        Ok(_) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            problems.push(format!("can't read {}: {e}", path.display()));
            None
        }
    }
}

fn read_text(path: &Path, problems: &mut Vec<String>) -> Option<String> {
    let bytes = read(path, problems)?;
    match String::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(_) => {
            problems.push(format!("{} isn't text", path.display()));
            None
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use nova_test_support::TempDir;

    /// A data folder with a small MIME database: names, magic, an alias
    /// and parents, like the real one's.
    pub fn data_dir(name: &str) -> TempDir {
        let dir = TempDir::new(name);
        let mime = dir.join("mime");
        std::fs::create_dir_all(&mime).unwrap();
        std::fs::write(
            mime.join("globs2"),
            "50:image/png:*.png\n50:application/pdf:*.pdf\n50:text/x-python:*.py\n50:text/markdown:*.md\n\
             50:video/mp4:*.mp4\n50:audio/mp4:*.mp4\n50:application/zip:*.zip\n\
             50:application/msword:*.doc\n50:text/plain:*.doc\n",
        )
        .unwrap();
        let magic = magic::tests::magic_file(&[
            (50, "image/png", &[(0, 0, b"\x89PNG", None, 1)]),
            (50, "application/pdf", &[(0, 0, b"%PDF-", None, 1)]),
            (50, "audio/mp4", &[(0, 4, b"ftypM4A", None, 1)]),
            (50, "video/mp4", &[(0, 4, b"ftypisom", None, 1)]),
            (50, "application/x-ole-storage", &[(0, 0, b"\xd0\xcf\x11\xe0", None, 1)]),
        ]);
        std::fs::write(mime.join("magic"), magic).unwrap();
        std::fs::write(mime.join("aliases"), "application/x-pdf application/pdf\n").unwrap();
        std::fs::write(
            mime.join("subclasses"),
            "text/x-python text/x-script\ntext/markdown text/plain\napplication/x-user text/x-script\n\
             application/msword application/x-ole-storage\n",
        )
        .unwrap();
        dir
    }

    fn db(dir: &TempDir) -> MimeDb {
        let (db, problems) = MimeDb::load(&[dir.to_path_buf()]);
        assert!(problems.is_empty(), "{problems:?}");
        db
    }

    #[test]
    fn names_settle_most_files_and_contents_the_rest() {
        let dir = data_dir("mime-type-of");
        let db = db(&dir);
        let files = dir.join("files");
        std::fs::create_dir_all(&files).unwrap();
        let write = |name: &str, bytes: &[u8]| {
            std::fs::write(files.join(name), bytes).unwrap();
            db.type_of(&files.join(name)).unwrap()
        };
        assert_eq!(write("photo.png", b"not even a png"), "image/png", "a single name match decides");
        assert_eq!(write("photo", b"\x89PNG...."), "image/png", "no name match: contents");
        assert_eq!(write("song.mp4", b"....ftypM4A "), "audio/mp4", "a tie between names: contents");
        assert_eq!(write("clip.mp4", b"????????????"), "video/mp4", "a tie no contents settle: the first");
        assert_eq!(
            write("report.doc", b"\xd0\xcf\x11\xe0...."),
            "application/msword",
            "the name's kind of what's in it"
        );
        assert_eq!(write("notes", b"just words\n"), TEXT);
        assert_eq!(write("blob", b"\x00\x01\x02"), BINARY);
        assert_eq!(write("empty", b""), EMPTY);
        assert_eq!(db.type_of(&files).unwrap(), DIRECTORY);
        assert!(db.type_of(&files.join("missing")).is_err());
    }

    #[test]
    fn listing_types_by_name_never_reads_the_file() {
        let dir = data_dir("mime-by-name");
        let db = db(&dir);
        let file = dir.join("unreadable.png");
        std::fs::write(&file, b"x").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
        let metadata = std::fs::metadata(&file).unwrap();
        assert_eq!(db.type_by_name("unreadable.png", &metadata), "image/png");
        assert_eq!(db.type_by_name("unknown", &metadata), BINARY);
        assert_eq!(db.type_by_name("folder", &std::fs::metadata(&*dir).unwrap()), DIRECTORY);
    }

    #[test]
    fn types_know_their_parents_and_old_names() {
        let dir = data_dir("mime-parents");
        let db = db(&dir);
        assert_eq!(db.with_parents("text/x-python"), ["text/x-python", "text/x-script", TEXT, BINARY]);
        assert_eq!(db.with_parents("text/markdown"), ["text/markdown", TEXT, BINARY]);
        assert_eq!(db.with_parents("application/x-pdf"), ["application/pdf", BINARY]);
        assert_eq!(db.with_parents(DIRECTORY), [DIRECTORY]);
        assert!(db.is_a("application/x-user", TEXT), "a kind of a text type is text too");
        assert!(db.is_a("text/x-python", TEXT));
        assert!(!db.is_a(TEXT, "text/x-python"));
    }

    #[test]
    fn reads_the_real_database_when_there_is_one() {
        let system = PathBuf::from("/usr/share");
        if !system.join("mime/globs2").exists() {
            return;
        }
        let (db, problems) = MimeDb::load(&[system]);
        assert!(problems.is_empty(), "{problems:?}");
        let metadata = std::fs::metadata("/usr/share/mime/globs2").unwrap();
        assert_eq!(db.type_by_name("photo.JPG", &metadata), "image/jpeg");
        assert_eq!(db.type_by_name("archive.tar.gz", &metadata), "application/x-compressed-tar");
        assert_eq!(db.type_of(Path::new("/usr/share/mime/magic")).unwrap(), BINARY);
    }
}
