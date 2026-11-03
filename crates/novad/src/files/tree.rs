//! The sidebar's folder tree (`files_tree`) and its places
//! (`files_places`): the folders a window's sidebar shows, worked out here
//! so the window only draws rows.
//!
//! The tree follows the window: it starts at Home (or at `/` for a folder
//! outside Home, like the address bar) and is open down to the window's
//! folder, so that folder always shows and is marked; the top is always
//! open. Each open folder
//! lists its subfolders in the file list's name order
//! ([`folder::natural`]). Hidden folders show only on the way to the
//! window's folder, unless the window shows hidden files. Folders the user
//! opened stay open wherever the window goes; a folder on the way the
//! user closed stays closed until the window moves on (the window keeps
//! both lists and sends them each time).
//!
//! Paths and names go both ways as wire text (`nova_protocol::wire_text`),
//! so a folder whose name isn't UTF-8 shows and opens like any other.
//!
//! It's read anew on every request, off the connection's thread: only the
//! open folders are listed, and a closed one is read only until its first
//! subfolder (at most [`SCAN_LIMIT`] entries), to tell whether it gets a
//! chevron.

use super::folder;
use nova_protocol::{
    FILES_PATH_MAX_WIRE_BYTES, FILES_TREE_MAX_FOLDERS, FilePlace, FolderTree, PlaceKind, TreeRow, wire_path, wire_text,
};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

/// Entries read from a closed folder before it's taken to have
/// subfolders: a huge folder of files gets a chevron rather than a long
/// read.
const SCAN_LIMIT: usize = 1000;

/// What the tree is asked for: the window's folder and the folders the
/// user opened or closed.
pub struct Request<'a> {
    pub folder: &'a str,
    pub opened: &'a [String],
    pub closed: &'a [String],
    pub hidden: bool,
}

/// The tree for `request`, with `home` as Home.
pub fn tree(request: &Request, home: &Path) -> Result<FolderTree, String> {
    for list in [request.opened, request.closed] {
        if list.len() > FILES_TREE_MAX_FOLDERS {
            return Err(format!("more than {FILES_TREE_MAX_FOLDERS} folders opened or closed"));
        }
    }
    let folder = resolve(request.folder, home)?;
    if !folder.is_dir() {
        return Err(format!("{} isn't a folder", folder.display()));
    }
    let root = if folder.starts_with(home) { home.to_path_buf() } else { PathBuf::from("/") };
    let tree = Tree {
        way: folder.ancestors().filter(|ancestor| ancestor.starts_with(&root)).map(Path::to_path_buf).collect(),
        folder: folder.clone(),
        opened: request.opened.iter().map(|path| wire_path(path)).collect(),
        closed: request.closed.iter().map(|path| wire_path(path)).collect(),
        hidden: request.hidden,
    };
    let mut rows = Vec::new();
    let name = root.file_name().map_or_else(|| "/".into(), wire_text);
    tree.add(&root, name, 0, false, &mut rows);
    Ok(FolderTree { folder: wire_text(&folder), rows })
}

/// The places that exist, in the sidebar's order: Home, then the user's
/// folders from `user-dirs.dirs`.
pub fn places(home: &Path, user_dir: impl Fn(&str, &str) -> PathBuf) -> Vec<FilePlace> {
    let folders = [
        (PlaceKind::Desktop, "DESKTOP", "Desktop"),
        (PlaceKind::Documents, "DOCUMENTS", "Documents"),
        (PlaceKind::Downloads, "DOWNLOAD", "Downloads"),
        (PlaceKind::Pictures, "PICTURES", "Pictures"),
    ];
    std::iter::once((PlaceKind::Home, home.to_path_buf()))
        .chain(folders.into_iter().map(|(kind, key, fallback)| (kind, user_dir(key, fallback))))
        .filter(|(_, path)| path.is_dir())
        .map(|(kind, path)| FilePlace { kind, path: wire_text(path) })
        .collect()
}

/// A typed or sent folder as an absolute path: `~` is Home, a trailing
/// slash is dropped, and `.` or `..` are refused, as for a window.
fn resolve(folder: &str, home: &Path) -> Result<PathBuf, String> {
    if folder.len() > FILES_PATH_MAX_WIRE_BYTES {
        return Err(format!("a path longer than {FILES_PATH_MAX_WIRE_BYTES} bytes"));
    }
    let path = match folder.strip_prefix('~') {
        Some("") => home.to_path_buf(),
        Some(rest) if rest.starts_with('/') => home.join(wire_path(rest.trim_start_matches('/'))),
        _ => wire_path(folder),
    };
    if !path.is_absolute() {
        return Err(format!("{folder} isn't an absolute path"));
    }
    if path.components().any(|component| matches!(component, Component::ParentDir | Component::CurDir)) {
        return Err(format!("{folder} has . or .. in it"));
    }
    Ok(path.components().collect())
}

struct Tree {
    /// The window's folder and every folder above it, up to the tree's top.
    way: HashSet<PathBuf>,
    folder: PathBuf,
    opened: HashSet<PathBuf>,
    closed: HashSet<PathBuf>,
    hidden: bool,
}

impl Tree {
    /// Adds the row for `path` and, while it's open, its subfolders' rows.
    /// `name` is wire text.
    fn add(&self, path: &Path, name: String, depth: u32, in_hidden: bool, rows: &mut Vec<TreeRow>) {
        let hidden = in_hidden || folder::is_hidden(&name);
        // The top is open too, so a window on Home shows what's in it.
        let leads_on = depth == 0 || (self.way.contains(path) && path != self.folder);
        let open = (leads_on && !self.closed.contains(path)) || self.opened.contains(path);
        let index = rows.len();
        rows.push(TreeRow { name, path: wire_text(path), depth, open, leaf: false, hidden });
        if !open {
            rows[index].leaf = !self.has_subfolders(path);
            return;
        }
        let children = self.subfolders(path);
        rows[index].leaf = children.is_empty();
        for child in children {
            self.add(&path.join(wire_path(&child)), child, depth + 1, hidden, rows);
        }
    }

    /// The subfolders of `path` the tree shows, as wire text in name order:
    /// the hidden ones only when they're on the way or hidden files show.
    fn subfolders(&self, path: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .filter(is_folder)
            .map(|entry| entry.file_name())
            .filter(|name| self.shows(path, name))
            .map(wire_text)
            .collect();
        // The way on shows even when its folder can't be listed.
        if let Some(next) = self.next_on_way(path)
            && !names.contains(&next)
        {
            names.push(next);
        }
        // Raw names break natural ties (`a` and `A`), so the order is the same
        // every time.
        names.sort_by(|a, b| folder::natural(a, b).then_with(|| a.cmp(b)));
        names
    }

    /// Whether `path` has a subfolder the tree would show, reading it only
    /// until the first one.
    fn has_subfolders(&self, path: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(path) else { return false };
        for (read, entry) in entries.flatten().enumerate() {
            if read == SCAN_LIMIT {
                return true;
            }
            if is_folder(&entry) && self.shows(path, &entry.file_name()) {
                return true;
            }
        }
        false
    }

    fn shows(&self, parent: &Path, name: &std::ffi::OsStr) -> bool {
        self.hidden || !name.as_encoded_bytes().starts_with(b".") || self.way.contains(&parent.join(name))
    }

    /// The name of the folder after `path` on the way to the window's
    /// folder, when `path` leads there.
    fn next_on_way(&self, path: &Path) -> Option<String> {
        let rest = self.folder.strip_prefix(path).ok()?;
        let next = rest.components().next()?;
        Some(wire_text(next.as_os_str()))
    }
}

/// A folder, or a link to one.
fn is_folder(entry: &std::fs::DirEntry) -> bool {
    match entry.file_type() {
        Ok(kind) if kind.is_symlink() => entry.path().is_dir(),
        Ok(kind) => kind.is_dir(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    /// A Home with folders, hidden ones among them, and a file.
    fn home(name: &str) -> TempDir {
        let dir = TempDir::new(&format!("files-tree-{name}"));
        for folder in [
            "Documents/Notes",
            "Pictures",
            "Projects 10",
            "Projects 2",
            "pictures",
            ".local/share/Steam/steamapps/common/Crimson Desert",
            ".cache/fonts",
        ] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
        }
        std::fs::write(dir.join("notes.txt"), b"").unwrap();
        dir
    }

    fn request(folder: &Path) -> String {
        folder.display().to_string()
    }

    fn ask(home: &Path, folder: &str, opened: &[&Path], closed: &[&Path], hidden: bool) -> Result<FolderTree, String> {
        let list = |paths: &[&Path]| paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>();
        let (opened, closed) = (list(opened), list(closed));
        tree(&Request { folder, opened: &opened, closed: &closed, hidden }, home)
    }

    /// Each row as `depth name` plus its marks: `+` open, `-` closed with
    /// subfolders, nothing for a leaf; `h` hidden.
    fn shape(tree: &FolderTree) -> Vec<String> {
        tree.rows
            .iter()
            .map(|row| {
                let state = if row.open {
                    "+"
                } else if row.leaf {
                    ""
                } else {
                    "-"
                };
                let hidden = if row.hidden { " h" } else { "" };
                format!("{} {}{state}{hidden}", row.depth, row.name)
            })
            .collect()
    }

    #[test]
    fn the_tree_opens_down_to_the_folder_in_hidden_ones_too() {
        let home = home("follows");
        let common = home.join(".local/share/Steam/steamapps/common");
        let tree = ask(&home, &request(&common), &[], &[], false).unwrap();
        let top = home.file_name().unwrap().to_string_lossy();
        assert_eq!(
            shape(&tree),
            [
                format!("0 {top}+"),
                "1 .local+ h".into(),
                "2 share+ h".into(),
                "3 Steam+ h".into(),
                "4 steamapps+ h".into(),
                "5 common- h".into(),
                "1 Documents-".into(),
                "1 Pictures".into(),
                "1 pictures".into(),
                "1 Projects 2".into(),
                "1 Projects 10".into(),
            ],
            "only the hidden folder on the way shows, names in the list's order"
        );
        assert_eq!(tree.folder, request(&common));
        assert_eq!(tree.rows[5].path, request(&common));
    }

    #[test]
    fn opened_folders_stay_open_and_closed_ones_on_the_way_close() {
        let home = home("toggles");
        let notes = home.join("Documents/Notes");
        let opened = ask(&home, &request(&home), &[&home.join("Documents")], &[], false).unwrap();
        assert!(shape(&opened).contains(&"2 Notes".to_string()));
        let closed = ask(&home, &request(&notes), &[], &[&home.join("Documents")], false).unwrap();
        assert!(shape(&closed).contains(&"1 Documents-".to_string()));
        assert!(!shape(&closed).contains(&"2 Notes".to_string()));
    }

    #[test]
    fn hidden_folders_show_everywhere_when_asked() {
        let home = home("hidden");
        let tree = ask(&home, &request(&home), &[], &[], true).unwrap();
        assert!(shape(&tree).contains(&"1 .cache- h".to_string()));
        assert!(shape(&tree).contains(&"1 .local- h".to_string()));
    }

    #[test]
    fn a_folder_outside_home_starts_at_the_root_and_home_is_typed_as_tilde() {
        let home = home("outside");
        let other = TempDir::new("files-tree-outside-other");
        let tree = ask(&home, &request(&other), &[], &[], false).unwrap();
        assert_eq!(tree.rows[0].name, "/");
        assert_eq!(tree.rows[0].path, "/");
        assert!(tree.rows.iter().any(|row| row.path == request(&other)), "the way down shows");
        let typed = ask(&home, "~/Documents/", &[], &[], false).unwrap();
        assert_eq!(typed.folder, request(&home.join("Documents")));
        assert_eq!(ask(&home, "~", &[], &[], false).unwrap().folder, request(&home));
    }

    #[test]
    fn what_isnt_a_folder_is_refused() {
        let home = home("refused");
        assert!(ask(&home, "Documents", &[], &[], false).unwrap_err().contains("absolute"));
        assert!(ask(&home, "~user", &[], &[], false).unwrap_err().contains("absolute"));
        assert!(ask(&home, "~/Documents/..", &[], &[], false).unwrap_err().contains(".."));
        assert!(ask(&home, "~/notes.txt", &[], &[], false).unwrap_err().contains("isn't a folder"));
        assert!(ask(&home, "~/Missing", &[], &[], false).unwrap_err().contains("isn't a folder"));
        let many = vec![&*home; FILES_TREE_MAX_FOLDERS + 1];
        assert!(ask(&home, "~", &many, &[], false).is_err());
    }

    #[test]
    fn a_folder_whose_name_isnt_utf8_shows_and_opens() {
        use std::os::unix::ffi::OsStrExt;
        let home = home("raw");
        let raw = home.join(std::ffi::OsStr::from_bytes(b"caf\xe9"));
        std::fs::create_dir_all(raw.join("inside")).unwrap();
        let tree = ask(&home, &request(&home), &[], &[], false).unwrap();
        let row = tree.rows.iter().find(|row| row.name == wire_text(raw.file_name().unwrap())).unwrap();
        assert!(!row.leaf);
        let inside = ask(&home, &row.path, &[], &[], false).unwrap();
        assert_eq!(inside.folder, wire_text(&raw));
        assert!(inside.rows.iter().any(|row| row.path == wire_text(&raw) && !row.leaf), "inside is seen");
    }

    #[test]
    fn places_are_the_folders_that_exist() {
        let home = home("places");
        let places = places(&home, |_, fallback| home.join(fallback));
        let kinds: Vec<_> = places.iter().map(|place| place.kind).collect();
        assert_eq!(kinds, [PlaceKind::Home, PlaceKind::Documents, PlaceKind::Pictures]);
        assert_eq!(places[1].path, request(&home.join("Documents")));
    }
}
