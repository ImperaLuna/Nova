//! The preview pane's previews (`files_preview`): what the selected file
//! looks like, worked out off the connection's thread. Its type comes from
//! the shared MIME database, reading its first bytes when the name doesn't
//! settle it ([`MimeDb::type_of`]), and says what it shows:
//! - an image is the file itself: the UI loads it, scaled, off its own
//!   thread, and shows what it is instead when Qt can't read it;
//! - text, and every type that's a kind of it (code, JSON, scripts), is its
//!   first lines ([`text`]);
//! - a PDF's first page (`pdftoppm`, from poppler) and a still from a video
//!   (`ffmpeg`'s thumbnail filter, which picks a telling frame near the
//!   start) are drawn once into the cache folder ([`Pictures`]), one at a
//!   time and only for the latest file asked about;
//! - anything else, or a picture whose program isn't installed or fails,
//!   is [`FilePreview::None`], and the pane shows what the file is.

use super::target::clean;
use crate::child;
use crate::lock;
use crate::mime::{Mime, MimeDb};
use nova_protocol::{
    FILES_PREVIEW_MAX_BYTES, FILES_PREVIEW_MAX_LINES, FILES_PREVIEW_PICTURE_PX, FilePreview, wire_text,
};
use sha1::Digest;
use std::collections::HashSet;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// What draws a PDF's first page.
const PDF_PROGRAM: &str = "pdftoppm";
/// What draws a video's still.
const VIDEO_PROGRAM: &str = "ffmpeg";
/// The longest a picture may take to draw: a broken or huge file gives up
/// rather than hold a thread.
const DRAW_TIMEOUT: Duration = Duration::from_secs(10);
/// Most pictures the cache keeps; past it, the least recently shown go.
/// As many files that couldn't be drawn are remembered, so a broken one
/// doesn't hold up the others each time it's selected.
const CACHE_KEEP: usize = 500;
/// How many bytes of the file's digest name its picture.
const CACHE_NAME_BYTES: usize = 8;
/// The PDF type, and the start of every image's and video's.
const PDF: &str = "application/pdf";
const IMAGE_PREFIX: &str = "image/";
const VIDEO_PREFIX: &str = "video/";
const PICTURE_EXTENSION: &str = "png";
/// The cache's mode, and its pictures': this user's only.
const PRIVATE_FOLDER: u32 = 0o700;
const PRIVATE_FILE: u32 = 0o600;

/// Works out previews.
pub struct Previews {
    mime: Arc<Mime>,
    pictures: Option<Pictures>,
}

impl Previews {
    /// Previews with types from `mime`, drawing pictures into `cache` with
    /// programs from `path_var`; none are drawn without a cache folder.
    pub fn new(mime: Arc<Mime>, cache: Option<PathBuf>, path_var: &str) -> Self {
        let pictures = cache.map(|folder| Pictures {
            folder,
            path_var: path_var.to_string(),
            drawing: Mutex::new(()),
            next_temp: AtomicU64::new(0),
            latest: AtomicU64::new(0),
            failed: Mutex::default(),
        });
        Self { mime, pictures }
    }

    /// What the preview pane shows for the file at `path` (absolute, wire
    /// text). It reads the file, and may draw it, so it runs off the
    /// connection's thread.
    pub fn preview(&self, path: &str) -> Result<FilePreview, String> {
        let file = clean(path)?;
        let db = self.mime.current();
        let mime = db.type_of(&file).map_err(|e| format!("can't read {}: {e}", file.display()))?;
        let kind = Kind::of(&db, &mime);
        Ok(match kind {
            Kind::Image => FilePreview::Image { path: wire_text(&file) },
            Kind::Text => text(&file).map_err(|e| format!("can't read {}: {e}", file.display()))?,
            Kind::Pdf | Kind::Video => {
                let drawn = self.pictures.as_ref().and_then(|pictures| pictures.picture(&file, kind));
                drawn.map_or(FilePreview::None, |picture| FilePreview::Image { path: wire_text(picture) })
            }
            Kind::Other => FilePreview::None,
        })
    }
}

/// How a type is previewed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Image,
    Text,
    Pdf,
    Video,
    Other,
}

impl Kind {
    fn of(db: &MimeDb, mime: &str) -> Self {
        // Before text: SVG is a kind of XML, which is a kind of text.
        if mime.starts_with(IMAGE_PREFIX) {
            Self::Image
        } else if mime.starts_with(VIDEO_PREFIX) {
            Self::Video
        } else if db.is_a(mime, PDF) {
            Self::Pdf
        } else if db.is_a(mime, crate::mime::TEXT) {
            Self::Text
        } else {
            Self::Other
        }
    }
}

/// The first lines of the text file at `file`: at most
/// [`FILES_PREVIEW_MAX_LINES`] and [`FILES_PREVIEW_MAX_BYTES`], with a
/// character cut off at the end dropped. One that turns out not to be text
/// (a NUL in it) has no preview.
fn text(file: &Path) -> std::io::Result<FilePreview> {
    let mut start = Vec::new();
    std::fs::File::open(file)?.take(FILES_PREVIEW_MAX_BYTES as u64 + 1).read_to_end(&mut start)?;
    let mut cut = start.len() > FILES_PREVIEW_MAX_BYTES;
    start.truncate(FILES_PREVIEW_MAX_BYTES);
    if start.contains(&0) {
        return Ok(FilePreview::None);
    }
    let valid = match std::str::from_utf8(&start) {
        Ok(_) => start.len(),
        // Only a character cut off at the very end is dropped; anything
        // else not UTF-8 shows as replacement characters.
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        Err(_) => start.len(),
    };
    let whole = String::from_utf8_lossy(&start[..valid]);
    let mut lines = whole.lines();
    let shown: Vec<&str> = lines.by_ref().take(FILES_PREVIEW_MAX_LINES).collect();
    cut |= lines.next().is_some();
    Ok(FilePreview::Text { text: shown.join("\n"), cut })
}

/// The pictures drawn for PDFs and videos, as PNGs in `folder` named by the
/// file's path, size and modification time, so a file changed since is
/// drawn anew. One is drawn at a time, so moving through a folder of PDFs
/// never starts a crowd of programs, and an ask overtaken by a later one
/// while it waited its turn gives up (the pane only shows the latest); one
/// already drawn is answered at once.
struct Pictures {
    folder: PathBuf,
    path_var: String,
    drawing: Mutex<()>,
    /// Numbers the temp files pictures are drawn into.
    next_temp: AtomicU64,
    /// Numbers the asks to draw, the latest last.
    latest: AtomicU64,
    /// The pictures that couldn't be drawn, by name: not tried again until
    /// the file changes.
    failed: Mutex<HashSet<String>>,
}

impl Pictures {
    /// The picture of `file` (a `kind` of PDF or video), drawn now unless
    /// it was before; none when it can't be.
    fn picture(&self, file: &Path, kind: Kind) -> Option<PathBuf> {
        let metadata = std::fs::metadata(file).ok()?;
        let name = cache_name(file, &metadata);
        let picture = self.folder.join(format!("{name}.{PICTURE_EXTENSION}"));
        if mark_shown(&picture) {
            return Some(picture);
        }
        if lock(&self.failed).contains(&name) {
            return None;
        }
        let ask = self.latest.fetch_add(1, Ordering::SeqCst) + 1;
        let _drawing = lock(&self.drawing);
        // Drawn while this one waited its turn.
        if mark_shown(&picture) {
            return Some(picture);
        }
        // Another file was asked about since: this one is no longer shown.
        if self.latest.load(Ordering::SeqCst) != ask {
            return None;
        }
        // Pictures show what's inside files that may be private.
        private_folder(&self.folder).ok()?;
        let temp = self.folder.join(format!(
            ".{}-{}.{}",
            std::process::id(),
            self.next_temp.fetch_add(1, Ordering::Relaxed),
            PICTURE_EXTENSION
        ));
        // Made private before the program writes into it, which keeps
        // the file's mode.
        let drawn = private_file(&temp).is_ok() && self.draw(file, kind, &temp);
        let kept = drawn && std::fs::rename(&temp, &picture).is_ok();
        let _ = std::fs::remove_file(&temp);
        if kept {
            self.trim();
        } else {
            let mut failed = lock(&self.failed);
            if failed.len() >= CACHE_KEEP {
                failed.clear();
            }
            failed.insert(name);
        }
        kept.then_some(picture)
    }

    /// Draws `file` into the PNG `out` with its kind's program, if it's
    /// installed; whether it did.
    fn draw(&self, file: &Path, kind: Kind, out: &Path) -> bool {
        let program = if kind == Kind::Pdf { PDF_PROGRAM } else { VIDEO_PROGRAM };
        let Some(binary) = nova_paths::find_program(program, &self.path_var) else { return false };
        let mut command = Command::new(binary);
        // Neither needs anything from novad's environment.
        command.env_clear();
        let px = FILES_PREVIEW_PICTURE_PX.to_string();
        if kind == Kind::Pdf {
            // `-singlefile` writes `<out base>.png`: the base is `out`
            // without its extension.
            command.args(["-png", "-f", "1", "-l", "1", "-singlefile", "-scale-to", &px]).arg(file);
            command.arg(out.with_extension(""));
        } else {
            // Never larger than the video itself.
            let scale = format!("thumbnail,scale='min({px},iw)':'min({px},ih)':force_original_aspect_ratio=decrease");
            command.args(["-nostdin", "-loglevel", "error", "-i"]).arg(file);
            command.args(["-vf", &scale, "-frames:v", "1", "-f", "image2", "-c:v", "png", "-y"]).arg(out);
        }
        child::output_within(command, DRAW_TIMEOUT).is_ok_and(|output| output.status.success()) && out.is_file()
    }

    /// Drops the pictures shown least recently past [`CACHE_KEEP`].
    fn trim(&self) {
        let Ok(entries) = std::fs::read_dir(&self.folder) else { return };
        let mut pictures: Vec<(SystemTime, PathBuf)> = entries
            .flatten()
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .filter(|entry| entry.path().extension().is_some_and(|extension| extension == PICTURE_EXTENSION))
            .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
            .collect();
        if pictures.len() <= CACHE_KEEP {
            return;
        }
        pictures.sort();
        for (_, old) in &pictures[..pictures.len() - CACHE_KEEP] {
            let _ = std::fs::remove_file(old);
        }
    }
}

/// What names `file`'s picture in the cache, in hex: a digest of its path,
/// size and modification time, and the size pictures are drawn at, the
/// same from one build of novad to the next.
fn cache_name(file: &Path, metadata: &std::fs::Metadata) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut digest = sha1::Sha1::new();
    digest.update(file.as_os_str().as_bytes());
    digest.update([0]);
    for number in [metadata.len(), metadata.mtime().cast_unsigned(), metadata.mtime_nsec().cast_unsigned()] {
        digest.update(number.to_le_bytes());
    }
    digest.update(FILES_PREVIEW_PICTURE_PX.to_le_bytes());
    digest.finalize()[..CACHE_NAME_BYTES].iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Makes `folder` (and the folders above it it makes) readable by this
/// user only.
fn private_folder(folder: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new().recursive(true).mode(PRIVATE_FOLDER).create(folder)?;
    // One made before, by hand or by an older novad.
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(PRIVATE_FOLDER))
}

/// Makes the empty file `file`, readable by this user only.
fn private_file(file: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::File::options().write(true).create_new(true).mode(PRIVATE_FILE).open(file).map(drop)
}

/// Whether the picture at `picture` exists, marking it shown now so the
/// cache keeps it longest.
fn mark_shown(picture: &Path) -> bool {
    let Ok(file) = std::fs::File::options().append(true).open(picture) else { return false };
    // Only orders what the cache drops first: the picture shows either way.
    let _ = file.set_modified(SystemTime::now());
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logger::Logger;
    use nova_protocol::LogLevel;
    use nova_test_support::{TempDir, write_program};

    /// Previews over the test MIME database, drawing with the fake
    /// programs in `<dir>/bin`, which log their arguments to `<dir>/drawn`
    /// and write a stand-in PNG where they're told with the shell's own
    /// printf (a test sandbox has no `cp`).
    struct Fixture {
        dir: TempDir,
        previews: Previews,
    }

    impl Fixture {
        fn new(name: &str, programs: &[&str]) -> Self {
            let dir = crate::mime::tests::data_dir(&format!("files-preview-{name}"));
            let hub = Arc::new(crate::hub::Hub::default());
            let logger = Arc::new(Logger::open(&dir.join("logs"), LogLevel::Debug, false, hub).unwrap());
            let mime = Mime::follow(vec![dir.to_path_buf()], logger);
            let bin = dir.join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let log = dir.join("drawn");
            for program in programs {
                // pdftoppm is told a base and adds `.png`; ffmpeg gets
                // the whole name last.
                let out = if *program == PDF_PROGRAM { "\"$last.png\"" } else { "\"$last\"" };
                let body = format!(
                    "for last; do :; done\necho \"{program} $*\" >> {}\nprintf '\\211PNG drawn' > {out}",
                    log.display()
                );
                write_program(&bin, program, &body);
            }
            let previews = Previews::new(mime, Some(dir.join("cache")), &bin.display().to_string());
            Self { dir, previews }
        }

        fn file(&self, name: &str, bytes: &[u8]) -> String {
            let path = self.dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            path.display().to_string()
        }

        fn draws(&self) -> usize {
            std::fs::read_to_string(self.dir.join("drawn")).map_or(0, |log| log.lines().count())
        }
    }

    fn image_path(preview: FilePreview) -> String {
        match preview {
            FilePreview::Image { path } => path,
            other => panic!("not a picture: {other:?}"),
        }
    }

    #[test]
    fn an_image_is_shown_as_itself() {
        let fixture = Fixture::new("image", &[]);
        let photo = fixture.file("photo.png", b"\x89PNG....");
        assert_eq!(fixture.previews.preview(&photo).unwrap(), FilePreview::Image { path: photo });
    }

    #[test]
    fn text_and_code_show_their_first_lines() {
        let fixture = Fixture::new("text", &[]);
        let script = fixture.file("tool.py", b"import sys\r\nprint(sys.argv)\n");
        assert_eq!(
            fixture.previews.preview(&script).unwrap(),
            FilePreview::Text { text: "import sys\nprint(sys.argv)".into(), cut: false },
            "a kind of text, by its name"
        );
        let notes = fixture.file("notes", b"just words\n");
        assert_eq!(
            fixture.previews.preview(&notes).unwrap(),
            FilePreview::Text { text: "just words".into(), cut: false },
            "no name to go by: its contents"
        );
        let long: String = (0..FILES_PREVIEW_MAX_LINES + 5).map(|line| format!("{line}\n")).collect();
        let FilePreview::Text { text, cut } =
            fixture.previews.preview(&fixture.file("long.md", long.as_bytes())).unwrap()
        else {
            panic!("long text is text");
        };
        assert!(cut);
        assert_eq!(text.lines().count(), FILES_PREVIEW_MAX_LINES);
    }

    #[test]
    fn a_text_file_cut_inside_a_character_drops_it() {
        let fixture = Fixture::new("cut-char", &[]);
        let mut bytes = vec![b'a'; FILES_PREVIEW_MAX_BYTES - 1];
        bytes.extend_from_slice("é and more".as_bytes());
        let FilePreview::Text { text, cut } = fixture.previews.preview(&fixture.file("big.md", &bytes)).unwrap() else {
            panic!("big text is text");
        };
        assert!(cut);
        assert_eq!(text.len(), FILES_PREVIEW_MAX_BYTES - 1, "the half of é is dropped");
        assert!(!text.contains('\u{fffd}'));
    }

    #[test]
    fn a_text_named_file_with_nul_bytes_has_no_preview() {
        let fixture = Fixture::new("nul", &[]);
        let fake = fixture.file("data.md", b"looks\0binary");
        assert_eq!(fixture.previews.preview(&fake).unwrap(), FilePreview::None);
    }

    #[test]
    fn a_pdf_and_a_video_are_drawn_once_into_the_cache() {
        let fixture = Fixture::new("drawn", &[PDF_PROGRAM, VIDEO_PROGRAM]);
        let report = fixture.file("report.pdf", b"%PDF-1.7");
        let page = image_path(fixture.previews.preview(&report).unwrap());
        assert!(page.starts_with(&fixture.dir.join("cache").display().to_string()), "{page}");
        assert_eq!(std::fs::read(&page).unwrap(), b"\x89PNG drawn");
        assert_eq!(image_path(fixture.previews.preview(&report).unwrap()), page, "kept");
        assert_eq!(fixture.draws(), 1, "drawn once");

        let clip = fixture.file("clip.mp4", b"....ftypisom");
        let still = image_path(fixture.previews.preview(&clip).unwrap());
        assert_ne!(still, page);
        assert_eq!(fixture.draws(), 2);
        let log = std::fs::read_to_string(fixture.dir.join("drawn")).unwrap();
        assert!(
            log.contains(&format!("pdftoppm -png -f 1 -l 1 -singlefile -scale-to {FILES_PREVIEW_PICTURE_PX} {report}"))
        );
        assert!(log.contains(&format!("ffmpeg -nostdin -loglevel error -i {clip} -vf thumbnail,")));

        // Changed since: drawn anew.
        std::fs::write(&report, b"%PDF-1.7 with a new page").unwrap();
        assert_ne!(image_path(fixture.previews.preview(&report).unwrap()), page);
        assert_eq!(fixture.draws(), 3);
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&fixture.dir.join("cache")), PRIVATE_FOLDER);
        assert_eq!(mode(Path::new(&page)), PRIVATE_FILE, "private from the start");
        let left: Vec<_> = std::fs::read_dir(fixture.dir.join("cache")).unwrap().flatten().collect();
        assert!(left.iter().all(|entry| !entry.file_name().to_string_lossy().starts_with('.')), "no temp files left");
    }

    #[test]
    fn a_file_that_cant_be_drawn_isnt_tried_again_until_it_changes() {
        let fixture = Fixture::new("failed", &[]);
        write_program(
            &fixture.dir.join("bin"),
            PDF_PROGRAM,
            &format!("echo tried >> {}\nexit 1", fixture.dir.join("drawn").display()),
        );
        let report = fixture.file("report.pdf", b"%PDF-1.7 broken");
        assert_eq!(fixture.previews.preview(&report).unwrap(), FilePreview::None);
        assert_eq!(fixture.previews.preview(&report).unwrap(), FilePreview::None);
        assert_eq!(fixture.draws(), 1, "remembered");
        std::fs::write(&report, b"%PDF-1.7 mended").unwrap();
        fixture.previews.preview(&report).unwrap();
        assert_eq!(fixture.draws(), 2, "changed: tried again");
    }

    #[test]
    fn without_its_program_a_pdf_has_no_preview() {
        let fixture = Fixture::new("no-program", &[]);
        let report = fixture.file("report.pdf", b"%PDF-1.7");
        assert_eq!(fixture.previews.preview(&report).unwrap(), FilePreview::None);
    }

    #[test]
    fn a_folder_or_another_type_has_no_preview() {
        let fixture = Fixture::new("other", &[]);
        let archive = fixture.file("photos.zip", b"PK\x03\x04");
        assert_eq!(fixture.previews.preview(&archive).unwrap(), FilePreview::None);
        assert_eq!(
            fixture.previews.preview(&fixture.dir.join("bin").display().to_string()).unwrap(),
            FilePreview::None
        );
    }

    #[test]
    fn a_relative_or_missing_path_is_refused() {
        let fixture = Fixture::new("refused", &[]);
        assert!(fixture.previews.preview("notes.txt").is_err());
        assert!(fixture.previews.preview(&fixture.dir.join("gone.md").display().to_string()).is_err());
    }
}
