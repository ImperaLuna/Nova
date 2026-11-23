//! The launcher's logic that draws nothing: what its list shows, where
//! each row sits and moving the selection. `ui/launcher/launcher.js` in today's shell.

use crate::tokens::{LABEL_HEIGHT, ROW_GAP, ROW_HEIGHT};
use nova_app_info::App;
use nova_launcher_api::{LauncherFile, LauncherItem, PROVIDER_APPS, RAW_BYTE_BASE, TextRun};
use nova_look::Glyph;
use std::collections::HashSet;

/// The headings of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Recent,
    Apps,
    Results,
    Files,
}

impl Group {
    pub fn heading(self) -> &'static str {
        match self {
            Self::Recent => "Recent",
            Self::Apps => "All apps",
            Self::Results => "Results",
            Self::Files => "Files",
        }
    }
}

/// One row of the list: an app, or what a provider found.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The provider that runs it, and what it is to that provider.
    pub provider: String,
    pub id: String,
    pub name: String,
    /// The name in runs, the letters a search matched marked.
    pub runs: Vec<TextRun>,
    pub description: String,
    /// An absolute path to its icon.
    pub icon: Option<String>,
    /// What it is when it's a file or folder.
    pub file: Option<LauncherFile>,
    pub group: Group,
}

impl Row {
    fn app(app: &App, group: Group) -> Self {
        Self {
            provider: PROVIDER_APPS.into(),
            id: app.id.clone(),
            name: app.name.clone(),
            runs: vec![TextRun { text: app.name.clone(), matched: false }],
            description: app.description.clone().unwrap_or_default(),
            icon: app.icon.clone(),
            file: None,
            group,
        }
    }

    fn found(item: &LauncherItem, group: Group) -> Self {
        Self {
            provider: item.provider.clone(),
            id: item.id.clone(),
            name: item.name.iter().map(|run| run.text.as_str()).collect(),
            runs: item.name.clone(),
            description: item.description.clone().unwrap_or_default(),
            icon: item.icon.clone(),
            file: item.file.clone(),
            group,
        }
    }
}

/// The apps with these ids, in their order, skipping any not in `apps`.
pub fn pick<'a>(apps: &'a [App], ids: &[String]) -> Vec<&'a App> {
    ids.iter().filter_map(|id| apps.iter().find(|app| app.id == *id)).collect()
}

/// What the list shows: what the providers found while there's a query,
/// else the recent apps followed by every other app by name, so the arrows
/// reach any app without typing; in the Files mode (`files`), only what was
/// found.
pub fn rows(query: &str, matches: &[LauncherItem], recent: &[String], apps: &[App], files: bool) -> Vec<Row> {
    if query.trim().is_empty() {
        if files {
            return Vec::new();
        }
        let shown: HashSet<&str> = recent.iter().map(String::as_str).collect();
        let mut others: Vec<&App> = apps.iter().filter(|app| !shown.contains(app.id.as_str())).collect();
        others.sort_by_cached_key(|app| app.name.to_lowercase());
        let recent = pick(apps, recent).into_iter().map(|app| Row::app(app, Group::Recent));
        return recent.chain(others.into_iter().map(|app| Row::app(app, Group::Apps))).collect();
    }
    let group = if files { Group::Files } else { Group::Results };
    matches.iter().map(|item| Row::found(item, group)).collect()
}

/// The selection after moving `by` rows from `index` among `count`, going
/// round at either end.
pub fn step(index: usize, count: usize, by: isize) -> usize {
    if count == 0 {
        return 0;
    }
    (index as isize + by).rem_euclid(count as isize) as usize
}

/// The query starts a path (`~` or `/`), which the Files mode completes.
pub fn types_path(query: &str) -> bool {
    query.starts_with('~') || query.starts_with('/')
}

/// What Tab would add to `query` to complete it to `row`'s path: the rest
/// of its completion when the query starts it (case aside), else "".
pub fn completion<'a>(query: &str, row: Option<&'a Row>) -> &'a str {
    let Some(full) = row.and_then(|row| row.file.as_ref()).map(|file| file.completion.as_str()) else {
        return "";
    };
    let mut rest = full.chars();
    let starts = query.chars().all(|typed| rest.next().is_some_and(|theirs| same_letter(typed, theirs)));
    if starts { rest.as_str() } else { "" }
}

fn same_letter(one: char, other: char) -> bool {
    one == other || one.to_lowercase().eq(other.to_lowercase())
}

/// `text` (a name or path from novad, wire text) for people: each run of
/// raw bytes shows as one replacement character, as other apps show such
/// names.
pub fn shown(text: &str) -> String {
    let raw = |character: char| (RAW_BYTE_BASE + 0x80..=RAW_BYTE_BASE + 0xFF).contains(&u32::from(character));
    let mut out = String::with_capacity(text.len());
    let mut in_raw = false;
    for character in text.chars() {
        let is_raw = raw(character);
        if !is_raw {
            out.push(character);
        } else if !in_raw {
            out.push(char::REPLACEMENT_CHARACTER);
        }
        in_raw = is_raw;
    }
    out
}

/// The line icon for a found file: a folder, a picture, a video, music, a
/// document with text, or a plain file.
pub fn file_glyph(file: &LauncherFile) -> Glyph {
    if file.dir {
        return Glyph::FOLDER;
    }
    match file.mime.split('/').next().unwrap_or_default() {
        "image" => Glyph::PICTURE,
        "video" => Glyph::VIDEO,
        "audio" => Glyph::MUSIC,
        "text" => Glyph::DOCUMENT,
        _ if file.mime == "application/pdf" => Glyph::DOCUMENT,
        _ => Glyph::FILE,
    }
}

/// Where the list's rows sit, in the list's own content: each under its
/// group's heading when it's the group's first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    /// Each item's top, heading included, and whether it has one.
    items: Vec<(f32, bool)>,
    height: f32,
}

impl Layout {
    pub fn of(rows: &[Row]) -> Self {
        let (label, row, gap) = (f32::from(LABEL_HEIGHT), f32::from(ROW_HEIGHT), f32::from(ROW_GAP));
        let mut items = Vec::with_capacity(rows.len());
        let mut top = 0.0;
        for (index, this) in rows.iter().enumerate() {
            let heads = index == 0 || rows[index - 1].group != this.group;
            items.push((top, heads));
            top += row + gap + if heads { label } else { 0.0 };
        }
        Self { items, height: (top - gap).max(0.0) }
    }

    /// The height of all of it.
    pub fn height(&self) -> f32 {
        self.height
    }

    /// Whether row `index` sits under a heading.
    pub fn heads(&self, index: usize) -> bool {
        self.items.get(index).is_some_and(|(_, heads)| *heads)
    }

    /// Where item `index` starts, its heading included.
    pub fn top(&self, index: usize) -> f32 {
        self.items.get(index).map_or(0.0, |(top, _)| *top)
    }

    /// Where row `index` itself starts, below its heading.
    pub fn row_top(&self, index: usize) -> f32 {
        self.top(index) + if self.heads(index) { f32::from(LABEL_HEIGHT) } else { 0.0 }
    }

    /// The furthest a view `shown` tall scrolls.
    pub fn furthest(&self, shown: f32) -> f32 {
        (self.height - shown).max(0.0)
    }

    /// The scroll offset that has item `index` in a view `shown` tall,
    /// moving as little as it can from `offset`.
    pub fn contain(&self, index: usize, offset: f32, shown: f32) -> f32 {
        let top = self.top(index);
        let bottom = self.row_top(index) + f32::from(ROW_HEIGHT);
        let offset = if top < offset { top } else { offset.max(bottom - shown) };
        offset.clamp(0.0, self.furthest(shown))
    }

    /// The items a view `shown` tall scrolled to `offset` shows any of.
    pub fn visible(&self, offset: f32, shown: f32) -> std::ops::Range<usize> {
        let ends = |index: usize| self.row_top(index) + f32::from(ROW_HEIGHT);
        let first = self.items.partition_point(|(top, heads)| {
            top + f32::from(ROW_HEIGHT) + if *heads { f32::from(LABEL_HEIGHT) } else { 0.0 } <= offset
        });
        let last = self.items.partition_point(|(top, _)| *top < offset + shown);
        debug_assert!(first >= last || ends(first) > offset);
        first..last.max(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str) -> App {
        App { id: id.into(), name: name.into(), description: None, icon: None }
    }

    fn item(id: &str, runs: &[(&str, bool)], file: Option<LauncherFile>) -> LauncherItem {
        LauncherItem {
            provider: if file.is_some() { "files".into() } else { PROVIDER_APPS.into() },
            id: id.into(),
            name: runs.iter().map(|(text, matched)| TextRun { text: (*text).into(), matched: *matched }).collect(),
            description: None,
            icon: None,
            file,
        }
    }

    fn file(completion: &str) -> LauncherFile {
        LauncherFile { dir: false, mime: "text/plain".into(), hidden: false, completion: completion.into() }
    }

    fn ids(rows: &[Row]) -> Vec<(&str, Group)> {
        rows.iter().map(|row| (row.id.as_str(), row.group)).collect()
    }

    #[test]
    fn without_a_query_it_lists_the_recent_apps_then_the_others_by_name() {
        let apps = [app("z", "Zed"), app("f", "firefox"), app("a", "Alacritty"), app("g", "GIMP")];
        let recent = ["g".to_owned(), "gone".to_owned(), "z".to_owned()];
        let rows = rows("  ", &[], &recent, &apps, false);
        assert_eq!(ids(&rows), [("g", Group::Recent), ("z", Group::Recent), ("a", Group::Apps), ("f", Group::Apps)]);
    }

    #[test]
    fn with_a_query_it_lists_what_was_found() {
        let found = [item("f", &[("Fire", true), ("fox", false)], None)];
        let rows = rows("fire", &found, &["z".to_owned()], &[app("z", "Zed")], false);
        assert_eq!(ids(&rows), [("f", Group::Results)]);
        assert_eq!(rows[0].name, "Firefox");
        assert!(rows[0].runs[0].matched);
    }

    #[test]
    fn the_files_mode_lists_only_what_was_found() {
        let found = [item("/home/me/a.txt", &[("a.txt", false)], Some(file("~/a.txt")))];
        assert!(rows("", &found, &[], &[app("z", "Zed")], true).is_empty());
        assert_eq!(ids(&rows("a", &found, &[], &[], true)), [("/home/me/a.txt", Group::Files)]);
    }

    #[test]
    fn the_selection_goes_round_at_either_end() {
        assert_eq!(step(0, 3, -1), 2);
        assert_eq!(step(2, 3, 1), 0);
        assert_eq!(step(1, 3, 1), 2);
        assert_eq!(step(5, 0, 1), 0);
    }

    #[test]
    fn tab_completes_a_typed_path_to_the_selected_file_s() {
        let found = rows("~/do", &[item("/home/me/Documents", &[], Some(file("~/Documents/")))], &[], &[], true);
        assert_eq!(completion("~/do", found.first()), "cuments/");
        assert_eq!(completion("~/Documents/", found.first()), "");
        assert_eq!(completion("~/x", found.first()), "");
        assert_eq!(completion("~/do", None), "");
        assert!(types_path("~") && types_path("/etc") && !types_path("etc/"));
    }

    #[test]
    fn raw_bytes_show_as_one_replacement_character_a_run() {
        let raw = |byte: u32| char::from_u32(RAW_BYTE_BASE + byte).unwrap();
        let name = format!("caf{}{}.md", raw(0xE9), raw(0xFF));
        assert_eq!(shown(&name), "caf\u{FFFD}.md");
        assert_eq!(shown("plain.md"), "plain.md");
    }

    #[test]
    fn a_file_s_icon_follows_its_type() {
        let of = |dir, mime: &str| file_glyph(&LauncherFile { dir, mime: mime.into(), ..file("") });
        assert_eq!(of(true, "inode/directory"), Glyph::FOLDER);
        assert_eq!(of(false, "image/png"), Glyph::PICTURE);
        assert_eq!(of(false, "application/pdf"), Glyph::DOCUMENT);
        assert_eq!(of(false, "application/zip"), Glyph::FILE);
    }

    #[test]
    fn rows_sit_under_their_group_s_heading() {
        let apps = [app("a", "A"), app("b", "B"), app("c", "C")];
        let layout = Layout::of(&rows("", &[], &["c".to_owned()], &apps, false));
        // Recent: heading 16 + row 46, gap 2; All apps: heading + two rows.
        assert!(layout.heads(0) && layout.heads(1) && !layout.heads(2));
        assert_eq!((layout.top(0), layout.row_top(0)), (0.0, 16.0));
        assert_eq!((layout.top(1), layout.row_top(1)), (64.0, 80.0));
        assert_eq!(layout.row_top(2), 128.0);
        assert_eq!(layout.height(), 174.0);
        assert_eq!(Layout::of(&[]).height(), 0.0);
    }

    #[test]
    fn the_list_scrolls_only_as_far_as_the_selection_needs() {
        let apps: Vec<App> = (0..10).map(|index| app(&format!("{index}"), &format!("App {index}"))).collect();
        let layout = Layout::of(&rows("", &[], &[], &apps, false));
        // One heading, ten rows: 16 + 10 * 46 + 9 * 2.
        assert_eq!(layout.height(), 494.0);
        assert_eq!(layout.contain(1, 0.0, 200.0), 0.0);
        // Row 5 ends at 16 + 5 * 48 + 46 = 302.
        assert_eq!(layout.contain(5, 0.0, 200.0), 102.0);
        assert_eq!(layout.contain(0, 102.0, 200.0), 0.0, "back to the heading");
        assert_eq!(layout.contain(9, 0.0, 200.0), 294.0);
        assert_eq!(layout.visible(0.0, 200.0), 0..4);
        assert_eq!(layout.visible(102.0, 200.0), 1..6);
        assert_eq!(layout.visible(294.0, 200.0), 5..10);
    }
}
