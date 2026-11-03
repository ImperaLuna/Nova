//! A file picker's file type filter as a folder view applies it
//! (`files_open`'s `filter`): a file shows when its name matches one of the
//! globs, case aside, or its type is one of the types, a kind of one
//! (`image/svg+xml` is a kind of `application/xml`), or under a `type/*`.
//! Folders always show, so the picker can still go anywhere.
//!
//! Globs are the shell's: `*` any run of characters, `?` one, `[abc]` or
//! `[a-z]` one of a set (`[!a]` or `[^a]` one not in it). Anything else,
//! or a set left open, matches itself.

use crate::mime::MimeDb;
use nova_protocol::{FILES_FILTER_MAX_PATTERNS, FILES_PICKER_MAX_CHARS, FileEntry, FileFilter};

/// A filter, checked and ready to match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    /// Lowercased, as characters.
    globs: Vec<Vec<char>>,
    mimes: Vec<String>,
}

impl Filter {
    /// `filter`, once it holds no more than a filter may.
    pub fn new(filter: &FileFilter) -> Result<Self, String> {
        if filter.globs.len() + filter.mimes.len() > FILES_FILTER_MAX_PATTERNS {
            return Err(format!("a filter holds at most {FILES_FILTER_MAX_PATTERNS} patterns"));
        }
        if filter.globs.iter().chain(&filter.mimes).any(|pattern| pattern.chars().count() > FILES_PICKER_MAX_CHARS) {
            return Err(format!("a filter's pattern is at most {FILES_PICKER_MAX_CHARS} characters"));
        }
        let globs = filter.globs.iter().map(|glob| glob.to_lowercase().chars().collect()).collect();
        Ok(Self { globs, mimes: filter.mimes.clone() })
    }

    /// Whether the view shows `entry`: a folder, or a file it matches.
    pub fn shows(&self, entry: &FileEntry, mime: &MimeDb) -> bool {
        if entry.dir {
            return true;
        }
        let name: Vec<char> = entry.name.to_lowercase().chars().collect();
        self.globs.iter().any(|glob| glob_matches(glob, &name))
            || self.mimes.iter().any(|wanted| type_matches(wanted, &entry.mime, mime))
    }
}

/// Whether the type `found` is `wanted`, a kind of it, or under its `type/*`.
fn type_matches(wanted: &str, found: &str, mime: &MimeDb) -> bool {
    match wanted.strip_suffix("/*") {
        Some(group) => found.split_once('/').is_some_and(|(found_group, _)| found_group.eq_ignore_ascii_case(group)),
        None => wanted.eq_ignore_ascii_case(found) || mime.is_a(found, wanted),
    }
}

/// Whether `name` matches all of `glob`. Iterative, with one star to go back
/// to, so a long name with many stars stays linear per star.
fn glob_matches(glob: &[char], name: &[char]) -> bool {
    let (mut at_glob, mut at_name) = (0, 0);
    // Where the last star was, and the name position it last took up to.
    let mut star: Option<(usize, usize)> = None;
    while at_name < name.len() {
        match glob.get(at_glob) {
            Some('*') => {
                star = Some((at_glob, at_name));
                at_glob += 1;
                continue;
            }
            Some(&token) => {
                let (matched, width) = one(glob, at_glob, token, name[at_name]);
                if matched {
                    at_glob += width;
                    at_name += 1;
                    continue;
                }
            }
            None => {}
        }
        match star {
            // The star takes one more character.
            Some((star_glob, star_name)) => {
                star = Some((star_glob, star_name + 1));
                at_glob = star_glob + 1;
                at_name = star_name + 1;
            }
            None => return false,
        }
    }
    glob[at_glob..].iter().all(|&token| token == '*')
}

/// Whether the glob's token at `at` (`token`) matches `character`, and how
/// many glob characters it took.
fn one(glob: &[char], at: usize, token: char, character: char) -> (bool, usize) {
    match token {
        '?' => (true, 1),
        '[' => match set(glob, at + 1, character) {
            Some((matched, end)) => (matched, end - at),
            // A set left open is a plain `[`.
            None => (character == '[', 1),
        },
        _ => (token == character, 1),
    }
}

/// The set starting at `start` (just past its `[`): whether `character` is
/// in it, and where the glob goes on after its `]`. `None` when it never
/// closes.
fn set(glob: &[char], start: usize, character: char) -> Option<(bool, usize)> {
    let negated = matches!(glob.get(start), Some('!' | '^'));
    let mut at = if negated { start + 1 } else { start };
    let first = at;
    let mut found = false;
    loop {
        let &token = glob.get(at)?;
        // A `]` first in the set is one of its characters.
        if token == ']' && at > first {
            return Some((found != negated, at + 1));
        }
        if glob.get(at + 1) == Some(&'-') && glob.get(at + 2).is_some_and(|&end| end != ']') {
            found |= (token..=glob[at + 2]).contains(&character);
            at += 3;
        } else {
            found |= token == character;
            at += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(glob: &str, name: &str) -> bool {
        let glob: Vec<char> = glob.to_lowercase().chars().collect();
        glob_matches(&glob, &name.to_lowercase().chars().collect::<Vec<_>>())
    }

    #[test]
    fn globs_match_like_a_shell_case_aside() {
        assert!(matches("*.png", "Cover.PNG"));
        assert!(matches("*.tar.gz", "backup.tar.gz"));
        assert!(!matches("*.png", "cover.png.txt"));
        assert!(matches("photo-??.jpg", "photo-12.jpg"));
        assert!(!matches("photo-??.jpg", "photo-1.jpg"));
        assert!(matches("*.[ch]", "main.c"));
        assert!(!matches("*.[!ch]", "main.c"));
        assert!(matches("report[0-9].pdf", "report7.pdf"));
        assert!(matches("*", ""));
        assert!(matches("a[b", "a[b"), "a set left open is plain text");
        assert!(matches("*a*a*a*a*b", &format!("{}b", "a".repeat(200))));
        assert!(!matches("*a*a*a*a*b", &"a".repeat(200)));
    }

    fn entry(name: &str, mime: &str, dir: bool) -> FileEntry {
        FileEntry {
            name: name.into(),
            dir,
            link: false,
            hidden: false,
            archive: false,
            mime: mime.into(),
            size: None,
            modified_ms: None,
        }
    }

    #[test]
    fn files_show_by_glob_or_type_and_folders_always() {
        let (db, _) = MimeDb::load(&[]);
        let filter = |globs: &[&str], mimes: &[&str]| {
            Filter::new(&FileFilter {
                name: "Pictures".into(),
                globs: globs.iter().map(|glob| (*glob).into()).collect(),
                mimes: mimes.iter().map(|mime| (*mime).into()).collect(),
            })
            .unwrap()
        };
        let images = filter(&[], &["image/*"]);
        assert!(images.shows(&entry("a.png", "image/png", false), &db));
        assert!(!images.shows(&entry("a.txt", "text/plain", false), &db));
        assert!(images.shows(&entry("Albums", "inode/directory", true), &db));
        let pdf = filter(&["*.pdf"], &["text/plain"]);
        assert!(pdf.shows(&entry("Report.PDF", "application/pdf", false), &db));
        assert!(pdf.shows(&entry("notes", "text/plain", false), &db));
        assert!(!pdf.shows(&entry("song.mp3", "audio/mpeg", false), &db));
    }

    #[test]
    fn oversized_filters_are_refused() {
        let many =
            FileFilter { name: "All".into(), globs: vec!["*".into(); FILES_FILTER_MAX_PATTERNS + 1], mimes: vec![] };
        assert!(Filter::new(&many).is_err());
        let long =
            FileFilter { name: "Long".into(), globs: vec!["x".repeat(FILES_PICKER_MAX_CHARS + 1)], mimes: vec![] };
        assert!(Filter::new(&long).is_err());
    }
}
