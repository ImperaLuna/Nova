//! The launcher's Files mode (`launcher.providers` view `files`), answered
//! by the Nova Files app: a query is a name search there, or, when it's a
//! path being typed (from `~` or `/`), what Nova Files completes it to from
//! that path's folder. Each path becomes an item with its type (for the icon), its folder spelled
//! from `~`, and whether it's hidden. Running an item asks Nova Files to
//! open it; Shift+Enter asks it to show the item in its folder (the user's
//! own file manager does both for folders in the `launcher` mode, see
//! `super`).
//!
//! Items are named by their absolute path, as wire text. A path that's gone
//! since Nova Files indexed it is left out. When Nova Files can't search,
//! the answer is one item saying why.
//!
//! Each item carries the path Tab completes to. For a typed path that's
//! spelled the way it was typed (`~/Doc` gives `~/Documents/`, a typed
//! `/home/me/Doc` gives `/home/me/Documents/`), so the launcher can show the
//! rest of it after what's typed.

use super::NovaFiles;
use crate::mime::{DIRECTORY, MimeDb};
use nova_launcher::providers::{Provider, Run};
use nova_launcher_api::{LauncherFile, LauncherItem, MAX_MATCHES, TextMark, TextRun, wire_path, wire_text};
use std::path::{Component, Path};

/// How a typed path names Home.
const HOME_PREFIX: char = '~';
const ROOT: char = '/';
const SEPARATOR: char = '/';
/// The type of a link whose target is gone.
const BROKEN_LINK_MIME: &str = "inode/symlink";
/// The icon of the item saying Nova Files couldn't search.
const UNANSWERED_ICON: &str = "dialog-error";

impl Provider for NovaFiles {
    fn own_mode(&self) -> bool {
        true
    }

    fn search(&self, query: &str) -> Result<Vec<LauncherItem>, String> {
        let limit = u32::try_from(MAX_MATCHES).unwrap_or(u32::MAX);
        let typed_path = query.starts_with(HOME_PREFIX) || query.starts_with(ROOT);
        let found = if typed_path { self.complete(query, limit) } else { NovaFiles::search(self, query, limit) };
        let paths = match found {
            Ok(paths) => paths,
            Err(e) => return Ok(vec![unanswered(e)]),
        };
        let mime = self.mime.current();
        let home = self.home.as_deref();
        let items = paths.iter().filter_map(|path| {
            if typed_path { completed(path, query, &mime, home) } else { item(path, query, &mime, home) }
        });
        Ok(items.take(MAX_MATCHES).collect())
    }

    fn run(&self, id: &str, how: &Run) -> Result<(), String> {
        let path = wire_path(id);
        // What Nova Files finds is absolute and plain; anything else wasn't
        // found by it.
        let plain = path.components().all(|part| matches!(part, Component::RootDir | Component::Normal(_)));
        if !path.is_absolute() || !plain {
            return Err(format!("{id} isn't a path Nova Files found"));
        }
        if how.new_window { self.show_in_folder(&path) } else { self.open(&path) }
    }
}

/// The launcher item for `path`, its name marked where `query`'s words are;
/// `None` when it's gone or names no file in a folder.
fn item(path: &Path, query: &str, mime: &MimeDb, home: Option<&Path>) -> Option<LauncherItem> {
    let (folder, name) = (path.parent()?, path.file_name()?);
    let own = std::fs::symlink_metadata(path).ok()?;
    let target = if own.file_type().is_symlink() { std::fs::metadata(path).ok() } else { Some(own) };
    let dir = target.as_ref().is_some_and(std::fs::Metadata::is_dir);
    let file_type = match &target {
        None => BROKEN_LINK_MIME,
        Some(_) if dir => DIRECTORY,
        Some(target) => mime.type_by_name(&name.to_string_lossy(), target),
    };
    let name = wire_text(name);
    let mut completion = typed(path, home);
    if dir {
        completion.push(SEPARATOR);
    }
    Some(LauncherItem {
        provider: String::new(),
        id: wire_text(path),
        name: runs(&name, &marks(query, &name)),
        description: Some(typed(folder, home)),
        icon: None,
        file: Some(LauncherFile {
            dir,
            mime: file_type.to_owned(),
            hidden: is_hidden(&name) || in_hidden_folder(folder, home),
            completion,
        }),
    })
}

/// The launcher item for `path`, which typed path `typed` completes to:
/// its name marked where it starts as typed, and its completion spelled
/// from what's typed up to the last `/` (`~` alone is Home's `~/`).
fn completed(path: &Path, typed: &str, mime: &MimeDb, home: Option<&Path>) -> Option<LauncherItem> {
    let (folder, start) = match typed.rsplit_once(SEPARATOR) {
        Some((folder, start)) => (format!("{folder}{SEPARATOR}"), start),
        None => (format!("{typed}{SEPARATOR}"), ""),
    };
    let mut item = item(path, start, mime, home)?;
    if let Some(file) = &mut item.file {
        file.completion = format!("{folder}{}", wire_text(path.file_name()?));
        if file.dir {
            file.completion.push(SEPARATOR);
        }
    }
    Some(item)
}

/// A dot file, hidden by default as on every Linux desktop.
fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
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

/// `path` as wire text the way it's typed: `~` for Home and what's in it.
fn typed(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(inside) if inside.as_os_str().is_empty() => HOME_PREFIX.to_string(),
        Some(inside) => format!("{HOME_PREFIX}{SEPARATOR}{}", wire_text(inside)),
        None => wire_text(path),
    }
}

/// Where each word of `query` first is in `name`, case aside, in UTF-16 code
/// units (what QML's strings count), in order and without overlaps. A word
/// Nova Files matched by its letters or a typo isn't in the name as typed,
/// so it marks nothing.
fn marks(query: &str, name: &str) -> Vec<TextMark> {
    // The name lowercased a character at a time, and where each of its
    // bytes came from in `name`.
    let mut folded = String::new();
    let mut origin = Vec::new();
    for (at, character) in name.char_indices() {
        for lower in character.to_lowercase() {
            folded.push(lower);
            origin.resize(folded.len(), at);
        }
    }
    let source = |folded_at: usize| origin.get(folded_at).copied().unwrap_or(name.len());
    let mut found: Vec<(usize, usize)> = query
        .split_whitespace()
        .filter_map(|word| {
            let word: String = word.chars().flat_map(char::to_lowercase).collect();
            let start = folded.find(&word)?;
            Some((source(start), source(start + word.len())))
        })
        .collect();
    found.sort_unstable();
    let units = |text: &str| u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX);
    let mut marks = Vec::new();
    let mut covered = 0;
    for (start, end) in found {
        let start = start.max(covered);
        if start >= end {
            continue;
        }
        covered = end;
        marks.push(TextMark { start: units(&name[..start]), length: units(&name[start..end]) });
    }
    marks
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

    fn shown(runs: &[TextRun]) -> Vec<(&str, bool)> {
        runs.iter().map(|run| (run.text.as_str(), run.matched)).collect()
    }

    #[test]
    fn each_word_of_the_query_is_marked_where_it_first_is() {
        let name = "Crimson Desert";
        assert_eq!(
            shown(&runs(name, &marks("des cri", name))),
            [("Cri", true), ("mson ", false), ("Des", true), ("ert", false)]
        );
        // A word matched by its letters or a typo marks nothing.
        assert_eq!(shown(&runs(name, &marks("crmsn", name))), [("Crimson Desert", false)]);
        // Overlapping words mark the stretch once.
        assert_eq!(marks("rim imso", name), [TextMark { start: 1, length: 3 }, TextMark { start: 4, length: 2 }]);
    }

    #[test]
    fn marks_count_utf16_and_fold_case_a_character_at_a_time() {
        let emoji = "🎮 Game";
        assert_eq!(marks("game", emoji), [TextMark { start: 3, length: 4 }]);
        assert_eq!(runs(emoji, &marks("game", emoji))[1], TextRun { text: "Game".into(), matched: true });
        // İ lowercases to two characters; the mark still covers the one.
        assert_eq!(marks("i̇stanbul", "İstanbul"), [TextMark { start: 0, length: 8 }]);
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
    fn only_dot_folders_below_home_hide_what_is_in_them() {
        let home = Path::new("/home/.me");
        assert!(!in_hidden_folder(&home.join("Documents"), Some(home)));
        assert!(in_hidden_folder(&home.join(".config/nova"), Some(home)));
        assert!(in_hidden_folder(Path::new("/srv/.cache"), Some(home)));
    }
}
