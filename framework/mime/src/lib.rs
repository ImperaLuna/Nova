mod follow;
mod globs;

pub use follow::Mime;

use globs::Globs;
use std::fs::Metadata;
use std::io::Read;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

pub const DIRECTORY: &str = "inode/directory";
pub const EMPTY: &str = "application/x-zerosize";
pub const BINARY: &str = "application/octet-stream";
const MAX_DB_FILE: u64 = 16 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct MimeDb {
    globs: Globs,
}

impl MimeDb {
    pub fn load(data_dirs: &[PathBuf]) -> (Self, Vec<String>) {
        let mut db = Self::default();
        let mut problems = Vec::new();
        for dir in data_dirs.iter().rev().map(|dir| dir.join("mime")) {
            if let Some(text) = read_text(&dir.join("globs2"), &mut problems) {
                db.globs.add(&text);
            }
        }
        (db, problems)
    }

    pub fn type_by_name<'a>(&'a self, name: &str, metadata: &Metadata) -> &'a str {
        if let Some(special) = special_type(metadata) {
            return special;
        }
        self.type_of_name(name, metadata.len())
    }

    pub fn type_of_name<'a>(&'a self, name: &str, size: u64) -> &'a str {
        if size == 0 {
            return EMPTY;
        }
        self.globs.find(name).types.first().copied().unwrap_or(BINARY)
    }
}

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

    pub fn data_dir(name: &str) -> TempDir {
        let dir = TempDir::new(name);
        let mime = dir.join("mime");
        std::fs::create_dir_all(&mime).unwrap();
        std::fs::write(
            mime.join("globs2"),
            "50:image/png:*.png\n50:application/pdf:*.pdf\n50:text/x-python:*.py\n50:text/markdown:*.md\n\
             50:video/mp4:*.mp4\n50:audio/mp4:*.mp4\n50:application/zip:*.zip\n",
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
    }
}
