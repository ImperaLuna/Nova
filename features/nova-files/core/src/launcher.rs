use super::NovaFiles;
use nova_launcher_api::{LauncherFile, LauncherItem, MAX_MATCHES, TextMark, TextRun, wire_path, wire_text};
use nova_mime::{DIRECTORY, MimeDb};
use std::path::{Component, Path};

const HOME_PREFIX: char = '~';
const ROOT: char = '/';
const SEPARATOR: char = '/';
const BROKEN_LINK_MIME: &str = "inode/symlink";
const UNANSWERED_ICON: &str = "dialog-error";

impl NovaFiles {
    pub fn items(&self, query: &str) -> Result<Vec<LauncherItem>, String> {
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

    pub fn run_item(&self, id: &str, new_window: bool) -> Result<(), String> {
        let path = wire_path(id);
        let plain = path.components().all(|part| matches!(part, Component::RootDir | Component::Normal(_)));
        if !path.is_absolute() || !plain {
            return Err(format!("{id} isn't a path Nova Files found"));
        }
        if new_window { self.show_in_folder(&path) } else { self.open(&path) }
    }
}

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

fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

fn in_hidden_folder(folder: &Path, home: Option<&Path>) -> bool {
    let below_home = home.and_then(|home| folder.strip_prefix(home).ok()).unwrap_or(folder);
    below_home.iter().any(|part| is_hidden(&part.to_string_lossy()))
}

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

fn typed(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(inside) if inside.as_os_str().is_empty() => HOME_PREFIX.to_string(),
        Some(inside) => format!("{HOME_PREFIX}{SEPARATOR}{}", wire_text(inside)),
        None => wire_text(path),
    }
}

fn marks(query: &str, name: &str) -> Vec<TextMark> {
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

fn runs(name: &str, marks: &[TextMark]) -> Vec<TextRun> {
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
        assert_eq!(shown(&runs(name, &marks("crmsn", name))), [("Crimson Desert", false)]);
        assert_eq!(marks("rim imso", name), [TextMark { start: 1, length: 3 }, TextMark { start: 4, length: 2 }]);
    }

    #[test]
    fn marks_count_utf16_and_fold_case_a_character_at_a_time() {
        let emoji = "🎮 Game";
        assert_eq!(marks("game", emoji), [TextMark { start: 3, length: 4 }]);
        assert_eq!(runs(emoji, &marks("game", emoji))[1], TextRun { text: "Game".into(), matched: true });
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
