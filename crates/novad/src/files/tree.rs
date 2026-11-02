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
//! It's read anew on every request, off the connection's thread: only the
//! open folders are listed, and a closed one is read only until its first
//! subfolder (at most [`SCAN_LIMIT`] entries), to tell whether it gets a
//! chevron.

use super::folder;
use nova_protocol::{FILES_PATH_MAX_BYTES, FILES_TREE_MAX_FOLDERS, FilePlace, FolderTree, PlaceKind, TreeRow};
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
        opened: request.opened.iter().map(PathBuf::from).collect(),
        closed: request.closed.iter().map(PathBuf::from).collect(),
        hidden: request.hidden,
    };
    let mut rows = Vec::new();
    let name = root.file_name().map_or_else(|| "/".into(), |name| name.to_string_lossy().into_owned());
    tree.add(&root, name, 0, false, &mut rows);
    Ok(FolderTree { folder: text(&folder)?, rows })
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
        .filter_map(|(kind, path)| Some(FilePlace { kind, path: path.to_str()?.to_owned() }))
        .collect()
}

/// A typed or sent folder as an absolute path: `~` is Home, a trailing
/// slash is dropped, and `.` or `..` are refused, as for a window.
fn resolve(folder: &str, home: &Path) -> Result<PathBuf, String> {
    if folder.len() > FILES_PATH_MAX_BYTES {
        return Err(format!("a path longer than {FILES_PATH_MAX_BYTES} bytes"));
    }
    let path = match folder.strip_prefix('~') {
        Some("") => home.to_path_buf(),
        Some(rest) if rest.starts_with('/') => home.join(rest.trim_start_matches('/')),
        _ => PathBuf::from(folder),
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
    fn add(&self, path: &Path, name: String, depth: u32, in_hidden: bool, rows: &mut Vec<TreeRow>) {
        let hidden = in_hidden || folder::is_hidden(&name);
        // The top is open too, so a window on Home shows what's in it.
        let leads_on = depth == 0 || (self.way.contains(path) && path != self.folder);
        let open = (leads_on && !self.closed.contains(path)) || self.opened.contains(path);
        let index = rows.len();
        let Some(wire_path) = path.to_str() else { return };
        rows.push(TreeRow { name, path: wire_path.to_owned(), depth, open, leaf: false, hidden });
        if !open {
            rows[index].leaf = !self.has_subfolders(path);
            return;
        }
        let children = self.subfolders(path);
        rows[index].leaf = children.is_empty();
        for child in children {
            self.add(&path.join(&child), child, depth + 1, hidden, rows);
        }
    }

    /// The subfolders of `path` the tree shows, in name order: the hidden
    /// ones only when they're on the way or hidden files show.
    fn subfolders(&self, path: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| is_folder(entry))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| self.shows(path, name))
            .collect();
        // The way on shows even when its folder can't be listed.
        if let Some(next) = self.next_on_way(path)
            && !names.contains(&next)
        {
            names.push(next);
        }
        names.sort_by(|a, b| folder::natural(a, b));
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
            if is_folder(&entry) && self.shows(path, &entry.file_name().to_string_lossy()) {
                return true;
            }
        }
        false
    }

    fn shows(&self, parent: &Path, name: &str) -> bool {
        self.hidden || !folder::is_hidden(name) || self.way.contains(&parent.join(name))
    }

    /// The name of the folder after `path` on the way to the window's
    /// folder, when `path` leads there.
    fn next_on_way(&self, path: &Path) -> Option<String> {
        let rest = self.folder.strip_prefix(path).ok()?;
        let next = rest.components().next()?;
        Some(next.as_os_str().to_string_lossy().into_owned())
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

/// `path` as wire text: it must be UTF-8, which every path the UI can show is.
fn text(path: &Path) -> Result<String, String> {
    path.to_str().map(str::to_string).ok_or_else(|| format!("{} isn't UTF-8", path.display()))
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
    fn places_are_the_folders_that_exist() {
        let home = home("places");
        let places = places(&home, |_, fallback| home.join(fallback));
        let kinds: Vec<_> = places.iter().map(|place| place.kind).collect();
        assert_eq!(kinds, [PlaceKind::Home, PlaceKind::Documents, PlaceKind::Pictures]);
        assert_eq!(places[1].path, request(&home.join("Documents")));
    }
}
