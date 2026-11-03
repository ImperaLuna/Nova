//! Recent files (`files_recent`, the Home page's Recent): the files any app
//! used last, from `$XDG_DATA_HOME/recently-used.xbel`, the list GTK, Qt
//! and other apps add to as they open files.
//!
//! The file is XBEL: one `<bookmark href="file:///…" added="…"
//! modified="…" visited="…">` element per file, times in ISO 8601 UTC.
//! Only those start tags are read, with their attributes' XML escapes
//! undone; nothing else in the file matters here. A file is used last at
//! the latest of its three times. Only local files that still exist are
//! listed, the latest first, at most [`FILES_RECENT_MAX`]; each is read
//! like a folder's entry, so it shows as the file list shows it.

use super::{folder, target};
use crate::mime::MimeDb;
use nova_protocol::{FILES_RECENT_MAX, RecentFile, wire_text};
use std::path::{Path, PathBuf};

/// The list, in the data folder.
pub const RECENT_FILE: &str = "recently-used.xbel";
/// A bookmark's start tag.
const BOOKMARK_TAG: &str = "<bookmark ";
/// The attributes a bookmark's times are in.
const TIMES: [&str; 3] = ["added", "modified", "visited"];

/// The recent files in `file`, typed by `mime`; none when it can't be read.
pub fn recent(file: &Path, mime: &MimeDb) -> Vec<RecentFile> {
    let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
    let mut used = bookmarks(&text);
    used.sort_by_key(|(_, used_ms)| std::cmp::Reverse(*used_ms));
    used.into_iter()
        .filter_map(|(path, used_ms)| {
            let (folder, name) = (path.parent()?, path.file_name()?);
            let entry = folder::read_entry(folder, name, mime)?;
            Some(RecentFile { folder: wire_text(folder), used_ms, entry })
        })
        .take(FILES_RECENT_MAX)
        .collect()
}

/// Each local file `text` lists, with when it was used last.
fn bookmarks(text: &str) -> Vec<(PathBuf, Option<i64>)> {
    text.split(BOOKMARK_TAG)
        .skip(1)
        .filter_map(|rest| {
            let tag = &rest[..rest.find('>')?];
            let path = target::path(&attribute(tag, "href")?).ok()?;
            let used_ms = TIMES.iter().filter_map(|name| attribute(tag, name).and_then(|time| millis(&time))).max();
            Some((path, used_ms))
        })
        .collect()
}

/// The value of attribute `name` in a start tag's text, its escapes undone.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let mut rest = tag;
    loop {
        let at = rest.find(name)?;
        let before = rest[..at].chars().next_back();
        let after = rest[at + name.len()..].trim_start();
        rest = &rest[at + name.len()..];
        let Some(value) = after.strip_prefix('=') else { continue };
        if before.is_some_and(|c| !c.is_whitespace()) {
            continue;
        }
        let value = value.trim_start();
        let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let value = &value[1..];
        return Some(unescape(&value[..value.find(quote)?]));
    }
}

/// `text` with XML's character and entity references undone; one it
/// doesn't know stays as written.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let reference = after.find(';').map(|end| (&after[..end], &after[end + 1..]));
        let decoded = reference.and_then(|(name, tail)| {
            let character = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => name
                    .strip_prefix("#x")
                    .map(|hex| u32::from_str_radix(hex, 16))
                    .or_else(|| name.strip_prefix('#').map(str::parse))
                    .and_then(Result::ok)
                    .and_then(char::from_u32),
            }?;
            Some((character, tail))
        });
        match decoded {
            Some((character, tail)) => {
                out.push(character);
                rest = tail;
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// An ISO 8601 time in milliseconds since 1970.
fn millis(time: &str) -> Option<i64> {
    time.parse::<jiff::Timestamp>().ok().map(|at| at.as_millisecond())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    fn list(dir: &Path, bookmarks: &[String]) -> PathBuf {
        let file = dir.join(RECENT_FILE);
        let body: String = bookmarks.concat();
        let text = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbel version=\"1.0\"\n xmlns:bookmark=\"http://www.freedesktop.org/standards/desktop-bookmarks\">\n{body}</xbel>\n"
        );
        std::fs::write(&file, text).unwrap();
        file
    }

    /// A bookmark as GLib writes one.
    fn bookmark(href: &str, modified: &str, visited: &str) -> String {
        format!(
            "  <bookmark href=\"{href}\" added=\"2026-01-01T09:00:00.000000Z\" modified=\"{modified}\" visited=\"{visited}\">\n    <info>\n      <metadata owner=\"http://freedesktop.org\">\n        <mime:mime-type type=\"text/plain\"/>\n      </metadata>\n    </info>\n  </bookmark>\n"
        )
    }

    #[test]
    fn the_latest_used_files_that_exist_come_first() {
        let dir = TempDir::new("files-recent-order");
        std::fs::create_dir_all(dir.join("Documents")).unwrap();
        std::fs::write(dir.join("Documents/plan & notes.md"), b"# plan").unwrap();
        std::fs::write(dir.join("photo one.png"), b"").unwrap();
        let file = list(
            &dir,
            &[
                bookmark(
                    &format!("file://{}/Documents/plan%20&amp;%20notes.md", dir.display()),
                    "2026-11-01T10:00:00Z",
                    "2026-11-02T08:30:00.123456Z",
                ),
                bookmark(&format!("file://{}/gone.txt", dir.display()), "2026-11-03T00:00:00Z", "2026-11-03T00:00:00Z"),
                bookmark("https://example.com/page", "2026-11-03T00:00:00Z", "2026-11-03T00:00:00Z"),
                bookmark(
                    &format!("file://{}/photo%20one.png", dir.display()),
                    "2026-11-02T09:00:00Z",
                    "2026-10-01T00:00:00Z",
                ),
            ],
        );
        let recent = recent(&file, &MimeDb::default());
        let names: Vec<_> = recent.iter().map(|file| file.entry.name.as_str()).collect();
        assert_eq!(names, ["photo one.png", "plan & notes.md"]);
        assert_eq!(recent[1].folder, wire_text(dir.join("Documents")));
        assert_eq!(recent[1].used_ms, Some(millis("2026-11-02T08:30:00.123Z").unwrap()));
        assert_eq!(recent[1].entry.size, Some(6));
    }

    #[test]
    fn at_most_the_limit_is_listed() {
        let dir = TempDir::new("files-recent-limit");
        let bookmarks: Vec<_> = (0..FILES_RECENT_MAX + 5)
            .map(|index| {
                std::fs::write(dir.join(format!("{index}.txt")), b"").unwrap();
                bookmark(
                    &format!("file://{}/{index}.txt", dir.display()),
                    "2026-11-01T10:00:00Z",
                    "2026-11-01T10:00:00Z",
                )
            })
            .collect();
        assert_eq!(recent(&list(&dir, &bookmarks), &MimeDb::default()).len(), FILES_RECENT_MAX);
    }

    #[test]
    fn no_list_lists_nothing() {
        let dir = TempDir::new("files-recent-none");
        assert!(recent(&dir.join(RECENT_FILE), &MimeDb::default()).is_empty());
    }

    #[test]
    fn attributes_and_escapes_are_read_as_xml_says() {
        let tag = r#"href='file:///a%20b' xhref="no" modified = "2026-11-01T10:00:00Z""#;
        assert_eq!(attribute(tag, "href").as_deref(), Some("file:///a%20b"));
        assert_eq!(attribute(tag, "modified").as_deref(), Some("2026-11-01T10:00:00Z"));
        assert_eq!(attribute(tag, "visited"), None);
        assert_eq!(unescape("a &amp; b &lt;c&gt; &#233;&#xE9; &bogus; &"), "a & b <c> éé &bogus; &");
    }
}
