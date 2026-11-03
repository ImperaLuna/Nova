//! The thumbnailers installed on the system: `thumbnailers/*.thumbnailer`
//! in each XDG data folder, as GNOME and Tumbler read them. Each names the
//! file types it takes (`MimeType=`) and how to run it (`Exec=`, with `%i`
//! the input path, `%u` its URI, `%o` the PNG to write and `%s` the size).
//! Images come from gdk-pixbuf's, videos from ffmpegthumbnailer's, and
//! whatever else the user installs is picked up too, so Nova Files has no
//! list of formats of its own.
//!
//! A file in an earlier data folder hides one of the same name in a later
//! one. A thumbnailer whose program isn't there (`TryExec`, or `Exec`'s
//! first word, looked for on the path and beside the data folder, as Nix
//! profiles install it) is skipped. For a type, one that names it exactly
//! wins over one that names a type it's a kind of.

use crate::apps::desktop_entry::{exec_words, keys_of, list, unescape};
use crate::mime::MimeDb;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const GROUP: &str = "[Thumbnailer Entry]";
const FOLDER: &str = "thumbnailers";
const EXTENSION: &str = "thumbnailer";
/// Thumbnailer files bigger than this aren't read; real ones are a few
/// lines.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// One thumbnailer.
#[derive(Debug, Clone, PartialEq)]
pub struct Thumbnailer {
    /// Its file's name, like `gdk-pixbuf-thumbnailer.thumbnailer`.
    pub name: String,
    /// The program, found.
    program: PathBuf,
    /// Its arguments, field codes not yet filled in.
    args: Vec<Arg>,
    mime_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Arg {
    Text(String),
    /// A word with field codes in it, quoted or not, as GNOME fills them.
    Codes(String),
}

/// What fills in a thumbnailer's field codes.
pub struct Job<'a> {
    pub input: &'a Path,
    pub uri: &'a str,
    pub output: &'a Path,
    pub size: u32,
}

impl Thumbnailer {
    /// The program and its arguments for `job`.
    pub fn command(&self, job: &Job) -> (PathBuf, Vec<OsString>) {
        let args = self
            .args
            .iter()
            .map(|arg| match arg {
                Arg::Text(text) => OsString::from(text),
                Arg::Codes(word) => fill(word, job),
            })
            .collect();
        (self.program.clone(), args)
    }
}

/// `word` with its field codes filled in from `job`; unknown ones are
/// dropped, as GNOME does.
fn fill(word: &str, job: &Job) -> OsString {
    let mut out = OsString::new();
    let mut text = String::new();
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => text.push('%'),
            Some('i') => {
                out.push(std::mem::take(&mut text));
                out.push(job.input.as_os_str());
            }
            Some('o') => {
                out.push(std::mem::take(&mut text));
                out.push(job.output.as_os_str());
            }
            Some('u') => text.push_str(job.uri),
            Some('s') => text.push_str(&job.size.to_string()),
            _ => {}
        }
    }
    out.push(text);
    out
}

/// Every usable thumbnailer in `data_dirs` (the most important first),
/// with programs looked for on `path_var`, and the problems met.
pub fn load(data_dirs: &[PathBuf], path_var: &str) -> (Vec<Thumbnailer>, Vec<String>) {
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    for dir in data_dirs.iter().map(|dir| dir.join(FOLDER)) {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == EXTENSION)
                && let Some(name) = path.file_name().and_then(|name| name.to_str())
            {
                files.entry(name.to_string()).or_insert(path);
            }
        }
    }
    let mut problems = Vec::new();
    let mut found = Vec::new();
    for (name, path) in files {
        match read(&name, &path, path_var) {
            Ok(Some(thumbnailer)) => found.push(thumbnailer),
            Ok(None) => {}
            Err(e) => problems.push(format!("{}: {e}", path.display())),
        }
    }
    (found, problems)
}

/// The thumbnailer for files of type `mime`, if one takes them.
pub fn for_type<'a>(thumbnailers: &'a [Thumbnailer], mime: &str, db: &MimeDb) -> Option<&'a Thumbnailer> {
    let mime = db.canonical(mime);
    let kinds = db.with_parents(mime);
    let takes = |thumbnailer: &&Thumbnailer, exact: bool| {
        thumbnailer.mime_types.iter().any(|taken| {
            let taken = db.canonical(taken);
            if exact { taken == mime } else { kinds.iter().any(|kind| kind == taken) }
        })
    };
    thumbnailers
        .iter()
        .find(|thumbnailer| takes(thumbnailer, true))
        .or_else(|| thumbnailers.iter().find(|thumbnailer| takes(thumbnailer, false)))
}

/// The thumbnailer described in `path`; none when its program isn't
/// installed.
fn read(name: &str, path: &Path, path_var: &str) -> Result<Option<Thumbnailer>, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_FILE_BYTES {
        return Err(format!("{size} bytes is too big for a thumbnailer"));
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let keys = keys_of(&text, GROUP);
    if keys.is_empty() {
        return Err(format!("no {GROUP} group"));
    }
    let exec = keys.get("Exec").ok_or("no Exec")?;
    let mut words = exec_words(&unescape(exec))?.into_iter();
    let (program, _) = words.next().ok_or("an empty Exec")?;
    // `share/thumbnailers/x.thumbnailer` installs its program in `bin/`.
    let beside = path.parent().and_then(Path::parent).and_then(Path::parent).map(|prefix| prefix.join("bin"));
    let search = std::env::join_paths(std::env::split_paths(path_var).chain(beside))
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|_| "a path that isn't text")?;
    if let Some(try_exec) = keys.get("TryExec").map(|program| unescape(program))
        && nova_paths::find_program(&try_exec, &search).is_none()
    {
        return Ok(None);
    }
    let Some(program) = nova_paths::find_program(&program, &search) else { return Ok(None) };
    let args = words.map(|(word, _)| if word.contains('%') { Arg::Codes(word) } else { Arg::Text(word) }).collect();
    let mime_types = keys.get("MimeType").map(|types| list(types)).unwrap_or_default();
    Ok(Some(Thumbnailer { name: name.to_string(), program, args, mime_types }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::{TempDir, write_program};

    /// gdk-pixbuf's, as it installs it.
    const GDK_PIXBUF: &str = "[Thumbnailer Entry]\nTryExec=gdk-pixbuf-thumbnailer\n\
        Exec=gdk-pixbuf-thumbnailer -s %s %u %o\nMimeType=image/png;image/jpeg;image/x-tga;\n";
    /// ffmpegthumbnailer's, as it installs it.
    const FFMPEG: &str = "[Thumbnailer Entry]\nTryExec=ffmpegthumbnailer\n\
        Exec=ffmpegthumbnailer -i %i -o %o -s %s -f\nMimeType=video/mp4;video/x-matroska;video/webm;\n";

    /// A data folder `share/` under `dir` with `thumbnailers` installed and
    /// their programs in `bin/` beside it.
    fn installed(dir: &TempDir, thumbnailers: &[(&str, &str)], programs: &[&str]) -> PathBuf {
        let share = dir.join("prefix/share");
        std::fs::create_dir_all(share.join(FOLDER)).unwrap();
        std::fs::create_dir_all(dir.join("prefix/bin")).unwrap();
        for (name, text) in thumbnailers {
            std::fs::write(share.join(FOLDER).join(format!("{name}.{EXTENSION}")), text).unwrap();
        }
        for program in programs {
            write_program(&dir.join("prefix/bin"), program, "exit 0");
        }
        share
    }

    fn db(dir: &TempDir) -> MimeDb {
        let data = dir.join("mime-data");
        std::fs::create_dir_all(data.join("mime")).unwrap();
        std::fs::write(data.join("mime/subclasses"), "image/x-tga image/x-icon-like\nvideo/webm video/x-matroska\n")
            .unwrap();
        std::fs::write(data.join("mime/aliases"), "image/jpg image/jpeg\n").unwrap();
        MimeDb::load(&[data]).0
    }

    #[test]
    fn programs_beside_the_data_folder_run_with_their_field_codes_filled_in() {
        let dir = TempDir::new("thumbnailers-exec");
        let share = installed(&dir, &[("gdk-pixbuf-thumbnailer", GDK_PIXBUF)], &["gdk-pixbuf-thumbnailer"]);
        let (found, problems) = load(&[share], "/nonexistent");
        assert!(problems.is_empty(), "{problems:?}");
        let job = Job {
            input: Path::new("/home/me/a b.png"),
            uri: "file:///home/me/a%20b.png",
            output: Path::new("/tmp/out.png"),
            size: 256,
        };
        let (program, args) = found[0].command(&job);
        assert_eq!(program, dir.join("prefix/bin/gdk-pixbuf-thumbnailer"));
        assert_eq!(args, ["-s", "256", "file:///home/me/a%20b.png", "/tmp/out.png"].map(OsString::from));
    }

    #[test]
    fn field_codes_in_quotes_are_filled_in_too() {
        let dir = TempDir::new("thumbnailers-quoted");
        let quoted =
            "[Thumbnailer Entry]\nExec=quoted-thumbnailer -s %s \"%i\" \"--out=%o\" \"100%%\"\nMimeType=image/png;\n";
        let share = installed(&dir, &[("quoted", quoted)], &["quoted-thumbnailer"]);
        let (found, _) = load(&[share], "/nonexistent");
        let (_, args) = found[0].command(&Job {
            input: Path::new("/p/a b.png"),
            uri: "file:///p/a%20b.png",
            output: Path::new("/o.png"),
            size: 256,
        });
        assert_eq!(args, ["-s", "256", "/p/a b.png", "--out=/o.png", "100%"].map(OsString::from));
    }

    #[test]
    fn one_whose_program_is_missing_is_skipped() {
        let dir = TempDir::new("thumbnailers-missing");
        let share = installed(
            &dir,
            &[("gdk-pixbuf-thumbnailer", GDK_PIXBUF), ("ffmpegthumbnailer", FFMPEG)],
            &["ffmpegthumbnailer"],
        );
        let (found, _) = load(&[share], "/nonexistent");
        assert_eq!(
            found.iter().map(|found| found.name.as_str()).collect::<Vec<_>>(),
            ["ffmpegthumbnailer.thumbnailer"]
        );
        let (program, args) = found[0].command(&Job {
            input: Path::new("/v/clip.mkv"),
            uri: "file:///v/clip.mkv",
            output: Path::new("/o.png"),
            size: 128,
        });
        assert!(program.ends_with("ffmpegthumbnailer"));
        assert_eq!(args, ["-i", "/v/clip.mkv", "-o", "/o.png", "-s", "128", "-f"].map(OsString::from));
    }

    #[test]
    fn an_earlier_data_folder_hides_a_later_ones_file_of_the_same_name() {
        let (first, second) = (TempDir::new("thumbnailers-first"), TempDir::new("thumbnailers-second"));
        let mine = "[Thumbnailer Entry]\nExec=my-thumbnailer %i %o\nMimeType=image/png;\n";
        let user = installed(&first, &[("gdk-pixbuf-thumbnailer", mine)], &["my-thumbnailer"]);
        let system = installed(&second, &[("gdk-pixbuf-thumbnailer", GDK_PIXBUF)], &["gdk-pixbuf-thumbnailer"]);
        let (found, _) = load(&[user, system], "/nonexistent");
        assert_eq!(found.len(), 1);
        assert!(found[0].program.ends_with("my-thumbnailer"));
    }

    #[test]
    fn a_type_goes_to_the_one_naming_it_before_one_naming_its_parent() {
        let dir = TempDir::new("thumbnailers-types");
        let parent_only = "[Thumbnailer Entry]\nExec=matroska-thumbnailer %i %o\nMimeType=video/x-matroska;\n";
        let share = installed(
            &dir,
            &[("a-matroska", parent_only), ("ffmpegthumbnailer", FFMPEG)],
            &["matroska-thumbnailer", "ffmpegthumbnailer"],
        );
        let (found, _) = load(&[share], "/nonexistent");
        let db = db(&dir);
        let name = |mime: &str| for_type(&found, mime, &db).map(|found| found.name.clone());
        assert_eq!(name("video/webm").as_deref(), Some("ffmpegthumbnailer.thumbnailer"));
        assert_eq!(name("text/plain"), None);
        let both = installed(&dir, &[("gdk-pixbuf-thumbnailer", GDK_PIXBUF)], &["gdk-pixbuf-thumbnailer"]);
        let (found, _) = load(&[both], "/nonexistent");
        assert_eq!(
            for_type(&found, "image/jpg", &db).map(|found| found.name.as_str()),
            Some("gdk-pixbuf-thumbnailer.thumbnailer"),
            "an alias counts as its type"
        );
    }

    #[test]
    fn a_broken_file_is_a_problem_not_a_thumbnailer() {
        let dir = TempDir::new("thumbnailers-broken");
        let share =
            installed(&dir, &[("no-group", "Exec=x\n"), ("no-exec", "[Thumbnailer Entry]\nMimeType=a/b;\n")], &[]);
        let (found, problems) = load(&[share], "/nonexistent");
        assert!(found.is_empty());
        assert_eq!(problems.len(), 2, "{problems:?}");
    }
}
