//! Quick access: the folders pinned to the top of Nova Files' sidebar and
//! its Home page, and the sidebar's places around them (`files_places`).
//!
//! Pins are the GTK bookmarks file (`$XDG_CONFIG_HOME/gtk-3.0/bookmarks`),
//! which GTK's file chooser and other file managers read and write too, so
//! a folder pinned here shows there and back. Each line is a URI and
//! maybe a label after a space; only `file://` lines are pins, and every
//! other line is kept as it is when Nova Files writes the file. Without the
//! file, the pins are the user's folders, as Explorer starts with them;
//! the first pin or unpin writes them out.
//!
//! It's read anew on every request, off the connection's thread, so a
//! change another app makes shows the next time a window asks (each time
//! the Home page shows). A line naming a folder twice is one pin. Pinning
//! writes a new file next to the old one and renames it over, under one
//! lock, so two windows pinning at once both land.

use super::target;
use super::trash::escape;
use nova_protocol::{FILES_HOME_PLACE, FILES_TRASH_PLACE, FilePlace, PlaceKind, wire_text};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The bookmarks file, in the config folder.
pub const BOOKMARKS_FILE: &str = "gtk-3.0/bookmarks";
/// A local folder's URI scheme in the bookmarks file.
const FILE_SCHEME: &str = "file://";

/// The user's folders, as Explorer's Quick access first pins them, by their
/// `user-dirs.dirs` key and the folder under Home when that isn't set.
pub const USER_FOLDERS: [(PlaceKind, &str, &str); 6] = [
    (PlaceKind::Desktop, "DESKTOP", "Desktop"),
    (PlaceKind::Downloads, "DOWNLOAD", "Downloads"),
    (PlaceKind::Documents, "DOCUMENTS", "Documents"),
    (PlaceKind::Pictures, "PICTURES", "Pictures"),
    (PlaceKind::Music, "MUSIC", "Music"),
    (PlaceKind::Videos, "VIDEOS", "Videos"),
];

pub struct QuickAccess {
    /// The bookmarks file; none without a config folder, when nothing is
    /// pinned and pinning fails.
    file: Option<PathBuf>,
    /// The user's folders, each with its kind.
    user_folders: Vec<(PlaceKind, PathBuf)>,
    /// Held while the file is read and written again.
    writing: Mutex<()>,
}

/// A line of the bookmarks file: a local folder, with its label, or any
/// other line, kept as it is.
enum Line {
    Pin { path: PathBuf, label: String, text: String },
    Other(String),
}

impl Line {
    fn parse(text: &str) -> Self {
        let (uri, label) = text.split_once(' ').unwrap_or((text, ""));
        // A path with `.` or `..` in it is no pin: one couldn't unpin it.
        let path = target::path(uri).and_then(|path| target::clean(&wire_text(&path)));
        match path {
            Ok(path) => Line::Pin { path, label: label.to_owned(), text: text.to_owned() },
            Err(_) => Line::Other(text.to_owned()),
        }
    }

    fn of(path: &Path) -> Self {
        let text = format!("{FILE_SCHEME}{}", escape(path.as_os_str()));
        Line::Pin { path: path.to_path_buf(), label: String::new(), text }
    }

    fn text(&self) -> &str {
        match self {
            Line::Pin { text, .. } | Line::Other(text) => text,
        }
    }
}

impl QuickAccess {
    /// Pins kept in `file`, starting from `user_folders` while it doesn't
    /// exist.
    pub fn new(file: Option<PathBuf>, user_folders: Vec<(PlaceKind, PathBuf)>) -> Self {
        Self { file, user_folders, writing: Mutex::default() }
    }

    /// The pinned folders that exist, in their order, each once.
    pub fn pins(&self) -> Result<Vec<FilePlace>, String> {
        let lines = self.read()?;
        let mut seen = std::collections::HashSet::new();
        let pins = lines.iter().filter_map(|line| match line {
            Line::Pin { path, label, .. } if path.is_dir() && seen.insert(path) => Some(self.pin_place(path, label)),
            _ => None,
        });
        Ok(pins.collect())
    }

    /// Pins the folder `folder` (wire text, absolute) or unpins it, and
    /// says whether that changed anything.
    pub fn pin(&self, folder: &str, pinned: bool) -> Result<bool, String> {
        let folder = target::clean(folder)?;
        if pinned && !folder.is_dir() {
            return Err(format!("{} isn't a folder", folder.display()));
        }
        let file = self.file.as_ref().ok_or("there's no config folder to keep pins in")?;
        let _writing = crate::lock(&self.writing);
        let mut lines = self.read()?;
        let is_it = |line: &Line| matches!(line, Line::Pin { path, .. } if *path == folder);
        let was = lines.iter().any(is_it);
        if was == pinned {
            return Ok(false);
        }
        if pinned {
            lines.push(Line::of(&folder));
        } else {
            lines.retain(|line| !is_it(line));
        }
        write(file, &lines).map_err(|e| format!("can't write {}: {e}", file.display()))?;
        Ok(true)
    }

    /// The file's lines, or the user's folders that exist while there's no
    /// file.
    fn read(&self) -> Result<Vec<Line>, String> {
        let defaults =
            || self.user_folders.iter().filter(|(_, path)| path.is_dir()).map(|(_, path)| Line::of(path)).collect();
        let Some(file) = &self.file else { return Ok(defaults()) };
        match std::fs::read_to_string(file) {
            Ok(text) => Ok(text.lines().filter(|line| !line.trim().is_empty()).map(Line::parse).collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(defaults()),
            Err(e) => Err(format!("can't read {}: {e}", file.display())),
        }
    }

    /// A pinned folder as the sidebar shows it: a user folder by its kind,
    /// named by `label` or else its own name.
    fn pin_place(&self, path: &Path, label: &str) -> FilePlace {
        let kind =
            self.user_folders.iter().find(|(_, folder)| folder == path).map_or(PlaceKind::Folder, |(kind, _)| *kind);
        let name = if label.is_empty() { path.file_name().map_or_else(|| "/".into(), wire_text) } else { label.into() };
        FilePlace { kind, path: wire_text(path), name }
    }
}

/// The sidebar's places, in its order: the Home page, `pins`, then the
/// trash, which is always there.
pub fn places(pins: Vec<FilePlace>) -> Vec<FilePlace> {
    let place = |kind, path: &str| FilePlace { kind, path: path.into(), name: String::new() };
    std::iter::once(place(PlaceKind::Home, FILES_HOME_PLACE))
        .chain(pins)
        .chain(std::iter::once(place(PlaceKind::Trash, FILES_TRASH_PLACE)))
        .collect()
}

/// Writes `lines` as `file`'s new contents: a new file renamed over it, so
/// a reader never sees half of it. A link is followed, so the file it
/// points to changes (or the write fails, for one in a read-only place like
/// a home-manager link), and the file keeps its permissions.
fn write(file: &Path, lines: &[Line]) -> std::io::Result<()> {
    let target = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    if let Some(folder) = target.parent() {
        std::fs::create_dir_all(folder)?;
    }
    // With no lines left, the empty file still says nothing is pinned.
    let text: String = lines.iter().map(|line| format!("{}\n", line.text())).collect();
    let mut new_name = target.as_os_str().to_owned();
    new_name.push(".nova-new");
    let new = PathBuf::from(new_name);
    std::fs::write(&new, text)?;
    if let Ok(old) = std::fs::metadata(&target) {
        std::fs::set_permissions(&new, old.permissions())?;
    }
    std::fs::rename(&new, &target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    fn home(name: &str) -> TempDir {
        let dir = TempDir::new(&format!("files-quick-access-{name}"));
        for folder in ["Desktop", "Documents", "Pictures", "Projects", "Old"] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
        }
        dir
    }

    fn quick_access(home: &Path) -> QuickAccess {
        let folders = USER_FOLDERS.iter().map(|(kind, _, fallback)| (*kind, home.join(fallback))).collect();
        QuickAccess::new(Some(home.join(".config").join(BOOKMARKS_FILE)), folders)
    }

    fn kinds(places: &[FilePlace]) -> Vec<PlaceKind> {
        places.iter().map(|place| place.kind).collect()
    }

    #[test]
    fn without_a_bookmarks_file_the_user_folders_that_exist_are_pinned() {
        let home = home("defaults");
        let listed = places(quick_access(&home).pins().unwrap());
        assert_eq!(
            kinds(&listed),
            [PlaceKind::Home, PlaceKind::Desktop, PlaceKind::Documents, PlaceKind::Pictures, PlaceKind::Trash]
        );
        assert_eq!(listed[0].path, FILES_HOME_PLACE);
        assert_eq!(listed[2].path, wire_text(home.join("Documents")));
        assert_eq!(listed[2].name, "Documents");
        assert_eq!(listed[4].path, FILES_TRASH_PLACE);
    }

    #[test]
    fn pins_are_read_from_and_written_to_the_shared_bookmarks_file() {
        let home = home("bookmarks");
        let file = home.join(".config").join(BOOKMARKS_FILE);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let projects = home.join("Projects");
        std::fs::write(
            &file,
            format!(
                "file://{} Work\nsftp://server/srv Server\nfile://{}\nfile://{}/Gone\n",
                escape(projects.as_os_str()),
                escape(home.join("Pictures").as_os_str()),
                home.display()
            ),
        )
        .unwrap();
        let quick = quick_access(&home);
        let listed = places(quick.pins().unwrap());
        assert_eq!(kinds(&listed), [PlaceKind::Home, PlaceKind::Folder, PlaceKind::Pictures, PlaceKind::Trash]);
        assert_eq!(listed[1].name, "Work", "the bookmark's label");

        assert!(quick.pin(&wire_text(home.join("Old")), true).unwrap());
        assert!(!quick.pin(&wire_text(home.join("Old")), true).unwrap(), "pinned already");
        assert!(quick.pin(&wire_text(&projects), false).unwrap());
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("sftp://server/srv Server\n"), "other apps' lines stay: {text}");
        assert!(text.contains("/Gone\n"), "a folder that's gone stays pinned for when it's back");
        assert!(!text.contains("Work"), "unpinned");
        let names: Vec<_> = places(quick.pins().unwrap()).into_iter().map(|place| place.name).collect();
        assert_eq!(names, ["", "Pictures", "Old", ""]);
    }

    #[test]
    fn the_first_change_writes_out_the_user_folders() {
        let home = home("first");
        let quick = quick_access(&home);
        assert!(quick.pin(&wire_text(home.join("Desktop")), false).unwrap());
        assert_eq!(
            kinds(&places(quick.pins().unwrap())),
            [PlaceKind::Home, PlaceKind::Documents, PlaceKind::Pictures, PlaceKind::Trash]
        );
        // Unpinning the rest leaves an empty file: nothing pinned, not the
        // user's folders again.
        quick.pin(&wire_text(home.join("Documents")), false).unwrap();
        quick.pin(&wire_text(home.join("Pictures")), false).unwrap();
        assert_eq!(kinds(&places(quick.pins().unwrap())), [PlaceKind::Home, PlaceKind::Trash]);
    }

    #[test]
    fn a_name_that_isnt_utf8_is_pinned_and_read_back() {
        use std::os::unix::ffi::OsStrExt;
        let home = home("raw");
        let raw = home.join(std::ffi::OsStr::from_bytes(b"caf\xe9 bar"));
        std::fs::create_dir_all(&raw).unwrap();
        let quick = quick_access(&home);
        assert!(quick.pin(&wire_text(&raw), true).unwrap());
        let text = std::fs::read_to_string(home.join(".config").join(BOOKMARKS_FILE)).unwrap();
        assert!(text.contains("caf%E9%20bar"), "{text}");
        let pinned = places(quick.pins().unwrap()).into_iter().find(|place| place.kind == PlaceKind::Folder).unwrap();
        assert_eq!(pinned.path, wire_text(&raw));
    }

    #[test]
    fn a_folder_twice_is_one_pin_and_dot_dot_is_none() {
        let home = home("twice");
        let file = home.join(".config").join(BOOKMARKS_FILE);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let old = escape(home.join("Old").as_os_str());
        std::fs::write(&file, format!("file://{old}\nfile://{old}/ Again\nfile://{old}/../Projects\n")).unwrap();
        let quick = quick_access(&home);
        assert_eq!(kinds(&places(quick.pins().unwrap())), [PlaceKind::Home, PlaceKind::Folder, PlaceKind::Trash]);
        assert!(quick.pin(&wire_text(home.join("Old")), false).unwrap());
        assert!(quick.pins().unwrap().is_empty(), "unpinning drops every line naming it");
    }

    #[test]
    fn a_linked_bookmarks_file_is_written_through_its_link_with_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let home = home("linked");
        let real = home.join("dotfiles/bookmarks");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        let file = home.join(".config").join(BOOKMARKS_FILE);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real, &file).unwrap();
        quick_access(&home).pin(&wire_text(home.join("Old")), true).unwrap();
        assert!(file.symlink_metadata().unwrap().file_type().is_symlink(), "the link stays");
        assert!(std::fs::read_to_string(&real).unwrap().contains("/Old"));
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn only_absolute_folders_are_pinned() {
        let home = home("refused");
        let quick = quick_access(&home);
        assert!(quick.pin("Projects", true).is_err());
        assert!(quick.pin(&wire_text(home.join("Projects/../Old")), true).is_err());
        assert!(quick.pin(&wire_text(home.join("Missing")), true).unwrap_err().contains("isn't a folder"));
    }
}
