//! The launcher's Files mode (`launcher.providers` view `files`): Nova
//! Files answering launcher searches. A query starting with `~` or `/` is a
//! path being typed: the folder it names so far is listed, the entries whose
//! names start with what follows its last `/` (case aside; dot entries only
//! once a dot is typed), folders first, each with the path Tab completes
//! to. Any other query searches the names Nova Files indexes (`search/`),
//! waiting for the index when it's still being built.
//!
//! Running an item opens a file in its type's default app and a folder in
//! Nova Files; Shift+Enter shows either in its folder, selected. Items are
//! named by their absolute path, as wire text, and checked again on run.
//!
//! While the `nova-files` feature runs, the standalone Nova Files app
//! (`crate::nova_files`) does the name searches and the index isn't asked.
//! When it can't, the answer is one item saying why, not the index's
//! names. What it found runs here as any other item does.

use super::Files;
use super::folder::{is_hidden, natural, read_entry};
use crate::launcher::MAX_MATCHES;
use crate::launcher::providers::{Provider, Run};
use crate::nova_files::NovaFiles;
use nova_protocol::{FileEntry, LauncherFile, LauncherItem, TextMark, TextRun, wire_path, wire_text};
use std::path::{Component, Path, PathBuf};

/// How long a name search waits for the index to be built, as Nova Files
/// builds it when it starts; past this it finds what there is (nothing).
const INDEX_WAIT: std::time::Duration = std::time::Duration::from_secs(10);
/// How a typed path names Home.
const HOME_PREFIX: char = '~';
const SEPARATOR: char = '/';
/// The icon of the item saying Nova Files couldn't search.
const UNANSWERED_ICON: &str = "dialog-error";

impl Provider for Files {
    fn own_mode(&self) -> bool {
        true
    }

    fn search(&self, query: &str) -> Result<Vec<LauncherItem>, String> {
        let home = self.home();
        match (TypedPath::parse(query, home), self.nova_files()) {
            (Some(typed), _) => typed.complete(&self.mime.current()),
            (None, Some(nova_files)) => Ok(self.found_by(&nova_files, query, home)),
            (None, None) => self.names(query, home),
        }
    }

    fn run(&self, id: &str, how: &Run) -> Result<(), String> {
        let path = super::target::clean(id)?;
        let folder = path.parent().ok_or_else(|| format!("{id} has no folder"))?;
        if how.new_window {
            return self.show(&wire_text(folder), &[wire_text(&path)]);
        }
        let id = wire_text(&path);
        if path.is_dir() { self.show(&id, &[]) } else { self.opener.open(&id) }
    }
}

impl Files {
    /// The user's Home, none when there's no home ("/").
    fn home(&self) -> Option<&Path> {
        Some(self.shared.sources.home.as_path()).filter(|home| *home != Path::new("/"))
    }

    /// The indexed files whose names hold every word of `query`, best first.
    fn names(&self, query: &str, home: Option<&Path>) -> Result<Vec<LauncherItem>, String> {
        let searcher = self.searcher();
        // Searches run off the connection's thread, so one typed while the
        // index is still being built waits for it rather than finding nothing.
        searcher.wait_ready(INDEX_WAIT);
        let found = searcher.search_up_to(query, None, MAX_MATCHES)?;
        Ok(found
            .results
            .into_iter()
            .map(|found| {
                let name = runs(&found.entry.name, &found.marks);
                let folder = wire_path(&found.folder);
                item(&folder, &found.entry, name, found.in_hidden_folder, home)
            })
            .collect())
    }
}

impl Files {
    /// What the Nova Files app finds for `query`, best first, or one item
    /// saying why it found nothing.
    fn found_by(&self, nova_files: &NovaFiles, query: &str, home: Option<&Path>) -> Vec<LauncherItem> {
        let limit = u32::try_from(MAX_MATCHES).unwrap_or(u32::MAX);
        let paths = match nova_files.search(query, limit) {
            Ok(paths) => paths,
            Err(e) => return vec![unanswered(e)],
        };
        let mime = self.mime.current();
        paths
            .iter()
            .filter_map(|path| {
                let (folder, name) = (path.parent()?, path.file_name()?);
                // Gone since it was indexed: nothing to open.
                let entry = read_entry(folder, name, &mime)?;
                let marked = runs(&entry.name, &super::search::marks(query, &entry.name));
                Some(item(folder, &entry, marked, in_hidden_folder(folder, home), home))
            })
            .take(MAX_MATCHES)
            .collect()
    }
}

/// Whether `folder`, below Home if it's there, is inside a dot folder. Only
/// folders below Home count: a Home inside a dot folder hides nothing.
fn in_hidden_folder(folder: &Path, home: Option<&Path>) -> bool {
    let below_home = home.and_then(|home| folder.strip_prefix(home).ok()).unwrap_or(folder);
    below_home.iter().any(|part| is_hidden(&part.to_string_lossy()))
}

/// The item that says a search found nothing because of `reason`. It names
/// no path, so running it does nothing.
fn unanswered(reason: String) -> LauncherItem {
    LauncherItem {
        provider: String::new(),
        id: String::new(),
        name: vec![TextRun { text: reason, matched: false }],
        description: None,
        icon: Some(UNANSWERED_ICON.to_string()),
        file: None,
    }
}

/// A path typed into the launcher: the folder it names so far, and the
/// start of a name in it.
struct TypedPath<'a> {
    folder: PathBuf,
    /// What's typed after the folder's last `/`.
    start: &'a str,
    /// Home, when the path was typed from `~`: the paths found are spelled
    /// from it too, so they complete what's typed.
    home: Option<&'a Path>,
}

impl<'a> TypedPath<'a> {
    /// `query` as a path: `~`, `~/…` (with a home) or `/…`; `None` for
    /// anything else, which is a name search.
    fn parse(query: &'a str, home: Option<&'a Path>) -> Option<Self> {
        let (base, rest, home) = match query.strip_prefix(HOME_PREFIX) {
            Some("") => (home?.to_path_buf(), "", home),
            Some(rest) => (home?.to_path_buf(), rest.strip_prefix(SEPARATOR)?, home),
            None => (PathBuf::from("/"), query.strip_prefix(SEPARATOR)?, None),
        };
        let (inside, start) = rest.rsplit_once(SEPARATOR).unwrap_or(("", rest));
        Some(Self { folder: base.join(wire_path(inside)), start, home })
    }

    /// The entries of the folder whose names start with `start`, folders
    /// first, then by name; none when the folder can't be read, or is named
    /// with a `..`, which Nova Files wouldn't open.
    fn complete(&self, mime: &crate::mime::MimeDb) -> Result<Vec<LauncherItem>, String> {
        if self.folder.components().any(|part| part == Component::ParentDir) {
            return Ok(Vec::new());
        }
        let Ok(entries) = std::fs::read_dir(&self.folder) else { return Ok(Vec::new()) };
        let start = self.start.to_lowercase();
        let mut names: Vec<(bool, String)> = entries
            .flatten()
            .filter_map(|entry| {
                let name = wire_text(entry.file_name());
                let shows = name.to_lowercase().starts_with(&start) && (!is_hidden(&name) || is_hidden(&start));
                // A link to a folder counts as one, as in its listing; only
                // links cost a look at the file, so a huge folder stays quick.
                let dir = |kind: std::fs::FileType| {
                    kind.is_dir()
                        || (kind.is_symlink() && std::fs::metadata(entry.path()).is_ok_and(|meta| meta.is_dir()))
                };
                shows.then(|| (entry.file_type().is_ok_and(dir), name))
            })
            .collect();
        names.sort_by(|(a_dir, a), (b_dir, b)| b_dir.cmp(a_dir).then_with(|| natural(a, b)).then_with(|| a.cmp(b)));
        let typed = self.start.chars().count();
        let hidden_folder = in_hidden_folder(&self.folder, self.home);
        Ok(names
            .into_iter()
            .take(MAX_MATCHES)
            .filter_map(|(_, name)| {
                let entry = read_entry(&self.folder, wire_path(&name).as_os_str(), mime)?;
                let split = entry.name.char_indices().nth(typed).map_or(entry.name.len(), |(at, _)| at);
                let (matched, rest) = entry.name.split_at(split);
                let name = [(matched, true), (rest, false)]
                    .into_iter()
                    .filter(|(text, _)| !text.is_empty())
                    .map(|(text, matched)| TextRun { text: text.to_string(), matched })
                    .collect();
                Some(item(&self.folder, &entry, name, hidden_folder, self.home))
            })
            .collect())
    }
}

/// The launcher item for `entry` in `folder`, its name in `name`'s runs.
fn item(
    folder: &Path,
    entry: &FileEntry,
    name: Vec<TextRun>,
    in_hidden_folder: bool,
    home: Option<&Path>,
) -> LauncherItem {
    let path = folder.join(wire_path(&entry.name));
    let mut completion = typed(&path, home);
    if entry.dir {
        completion.push(SEPARATOR);
    }
    LauncherItem {
        provider: String::new(),
        id: wire_text(&path),
        name,
        description: Some(typed(folder, home)),
        icon: None,
        file: Some(LauncherFile {
            dir: entry.dir,
            mime: entry.mime.clone(),
            hidden: entry.hidden || in_hidden_folder,
            completion,
        }),
    }
}

/// `path` as wire text the way it's typed: `~` for Home and what's in it.
fn typed(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(inside) if inside.as_os_str().is_empty() => HOME_PREFIX.to_string(),
        Some(inside) => format!("{HOME_PREFIX}{SEPARATOR}{}", wire_text(inside)),
        None => wire_text(path),
    }
}

/// `name` in runs, `marks` (UTF-16 code units, in order) matched.
fn runs(name: &str, marks: &[TextMark]) -> Vec<TextRun> {
    // Where each UTF-16 offset is in bytes.
    let byte_at = |unit: u32| {
        let mut units = 0;
        for (at, character) in name.char_indices() {
            if units >= unit {
                return at;
            }
            units += u32::try_from(character.len_utf16()).unwrap_or(u32::MAX);
        }
        name.len()
    };
    let mut runs = Vec::new();
    let mut push = |text: &str, matched: bool| {
        if !text.is_empty() {
            runs.push(TextRun { text: text.to_string(), matched });
        }
    };
    let mut done = 0;
    for mark in marks {
        let start = byte_at(mark.start).max(done);
        let end = byte_at(mark.start.saturating_add(mark.length)).max(start);
        push(&name[done..start], false);
        push(&name[start..end], true);
        done = end;
    }
    push(&name[done..], false);
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    fn texts(items: &[LauncherItem]) -> Vec<String> {
        items.iter().map(|item| item.name.iter().map(|run| run.text.as_str()).collect()).collect()
    }

    fn mime() -> crate::mime::MimeDb {
        crate::mime::MimeDb::default()
    }

    /// A home with a few folders and files in it.
    fn home(name: &str) -> TempDir {
        let dir = TempDir::new(&format!("files-launcher-{name}"));
        for folder in ["Documents", "Downloads", "Desktop", ".config", "Documents/Taxes"] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
        }
        for file in ["Documents/report 10.pdf", "Documents/report 2.pdf", "dotfile.txt", ".hidden"] {
            std::fs::write(dir.join(file), b"x").unwrap();
        }
        dir
    }

    #[test]
    fn a_typed_path_lists_the_entries_starting_with_what_follows_its_last_slash() {
        let home = home("complete");
        let complete = |query| TypedPath::parse(query, Some(&home)).unwrap().complete(&mime()).unwrap();
        assert_eq!(texts(&complete("~/d")), ["Desktop", "Documents", "Downloads", "dotfile.txt"]);
        let docs = complete("~/doc");
        assert_eq!(texts(&docs), ["Documents"]);
        let item = &docs[0];
        assert_eq!(item.name[0], TextRun { text: "Doc".into(), matched: true });
        assert_eq!(item.id, wire_text(home.join("Documents")));
        assert_eq!(item.description.as_deref(), Some("~"));
        let file = item.file.as_ref().unwrap();
        assert!(file.dir && !file.hidden);
        assert_eq!(file.completion, "~/Documents/");
        // Folders first, then names as people read them.
        assert_eq!(texts(&complete("~/Documents/")), ["Taxes", "report 2.pdf", "report 10.pdf"]);
        let report = &complete("~/Documents/report 2")[0];
        assert_eq!(report.file.as_ref().unwrap().completion, "~/Documents/report 2.pdf");
        assert_eq!(report.description.as_deref(), Some("~/Documents"));
        // Typed from `/`, it completes from `/`.
        let absolute = format!("{}/Doc", home.display());
        let docs = TypedPath::parse(&absolute, Some(&home)).unwrap().complete(&mime()).unwrap();
        assert_eq!(docs[0].file.as_ref().unwrap().completion, format!("{}/Documents/", home.display()));
    }

    #[test]
    fn a_link_to_a_folder_lists_as_a_folder() {
        let home = home("links");
        std::os::unix::fs::symlink(home.join("Documents"), home.join("zz docs")).unwrap();
        let found = TypedPath::parse("~/", Some(&home)).unwrap().complete(&mime()).unwrap();
        let link = found.iter().position(|item| item.name[0].text == "zz docs").unwrap();
        let file = found.iter().position(|item| item.name[0].text == "dotfile.txt").unwrap();
        assert!(link < file, "folders first");
        assert_eq!(found[link].file.as_ref().unwrap().completion, "~/zz docs/");
    }

    #[test]
    fn dot_entries_show_once_a_dot_is_typed_and_stay_dim() {
        let home = home("dots");
        let complete = |query| TypedPath::parse(query, Some(&home)).unwrap().complete(&mime()).unwrap();
        assert!(!texts(&complete("~/")).iter().any(|name| name.starts_with('.')));
        let dots = complete("~/.");
        assert_eq!(texts(&dots), [".config", ".hidden"]);
        assert!(dots.iter().all(|item| item.file.as_ref().unwrap().hidden));
    }

    #[test]
    fn only_home_and_absolute_paths_are_typed_paths() {
        let home = PathBuf::from("/home/me");
        let parse = |query| TypedPath::parse(query, Some(&home)).map(|typed| (typed.folder, typed.start.to_string()));
        assert_eq!(parse("~"), Some((home.clone(), String::new())));
        assert_eq!(parse("~/Doc"), Some((home.clone(), "Doc".into())));
        assert_eq!(parse("~/Documents/re"), Some((home.join("Documents"), "re".into())));
        assert_eq!(parse("/us"), Some((PathBuf::from("/"), "us".into())));
        assert_eq!(parse("/usr/share/"), Some((PathBuf::from("/usr/share"), String::new())));
        assert_eq!(parse("report"), None);
        assert_eq!(parse("~me"), None);
        assert!(TypedPath::parse("~/Doc", None).is_none(), "no home, no ~");
        let up = TypedPath::parse("~/../", Some(&home)).unwrap();
        assert!(up.complete(&mime()).unwrap().is_empty(), "Nova Files opens no path with a ..");
        // A folder that can't be read finds nothing.
        let missing = TypedPath::parse("/no/such/folder/x", None).unwrap();
        assert!(missing.complete(&mime()).unwrap().is_empty());
    }

    #[test]
    fn paths_read_from_home_with_a_tilde() {
        let home = Path::new("/home/me");
        assert_eq!(typed(home, Some(home)), "~");
        assert_eq!(typed(&home.join("Documents"), Some(home)), "~/Documents");
        assert_eq!(typed(Path::new("/mnt/usb"), Some(home)), "/mnt/usb");
        assert_eq!(typed(&home.join("Documents"), None), "/home/me/Documents");
    }

    #[test]
    fn marks_become_matched_runs() {
        let marks = [TextMark { start: 0, length: 3 }, TextMark { start: 8, length: 3 }];
        let found = runs("Crimson Desert", &marks);
        let shown: Vec<(&str, bool)> = found.iter().map(|run| (run.text.as_str(), run.matched)).collect();
        assert_eq!(shown, [("Cri", true), ("mson ", false), ("Des", true), ("ert", false)]);
        // UTF-16 offsets past a character outside the BMP.
        let emoji = runs("🎮 game", &[TextMark { start: 3, length: 4 }]);
        assert_eq!(emoji[1], TextRun { text: "game".into(), matched: true });
        assert_eq!(runs("plain", &[]), [TextRun { text: "plain".into(), matched: false }]);
    }
}
