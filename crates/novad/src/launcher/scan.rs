//! Where desktop entries live and reading them all: `applications/` in each
//! XDG data folder (the user's first), plus the folders packages install
//! into, which a systemd user service's `XDG_DATA_DIRS` often lacks.
//! The first folder with an id wins, as the spec says, so the user's own
//! copy of an entry replaces the system's.

use super::desktop_entry::{self, Context, Entry, Skipped};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The spec's default for an unset `XDG_DATA_DIRS`.
const DEFAULT_DATA_DIRS: &str = "/usr/local/share:/usr/share";
const ENTRY_EXTENSION: &str = "desktop";
/// Links followed when looking for what to watch, like the kernel's limit.
const MAX_LINKS: usize = 40;
/// Folders below `applications/` that are looked in; deeper ones aren't.
const MAX_DEPTH: usize = 8;
/// A desktop entry is a few kilobytes; anything far bigger isn't one.
const MAX_ENTRY_BYTES: u64 = 1024 * 1024;

/// The data folders, in the order they're searched.
pub fn data_dirs() -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    let home = var("HOME").map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = Vec::new();
    dirs.extend(
        var("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|home| home.join(".local/share"))),
    );
    let system = var("XDG_DATA_DIRS").unwrap_or_else(|| DEFAULT_DATA_DIRS.into());
    dirs.extend(std::env::split_paths(&system));
    // Where packages install, last so the session's own order wins.
    dirs.extend(crate::package_prefixes().into_iter().map(|prefix| prefix.join("share")));
    let mut seen = BTreeSet::new();
    dirs.retain(|dir| dir.is_absolute() && seen.insert(dir.clone()));
    dirs
}

/// Every app in `data_dirs`, by id, and the problems with entries that
/// aren't valid, for the log.
pub fn scan(data_dirs: &[PathBuf], context: &Context) -> (BTreeMap<String, Entry>, Vec<String>) {
    let mut apps = BTreeMap::new();
    // Ids already decided, shown or not: later folders can't bring them back.
    let mut decided = BTreeSet::new();
    let mut problems = Vec::new();
    for dir in data_dirs {
        let applications = dir.join("applications");
        let mut files = Vec::new();
        entry_files(&applications, &applications, 0, &mut files);
        for (id, file) in files {
            if !decided.insert(id.clone()) {
                continue;
            }
            let text = match read_entry(&file) {
                Ok(text) => text,
                Err(e) => {
                    problems.push(format!("can't read {}: {e}", file.display()));
                    continue;
                }
            };
            match desktop_entry::parse(&id, &file, &text, context) {
                Ok(entry) => {
                    apps.insert(id, entry);
                }
                Err(Skipped::Invalid(why)) => problems.push(format!("skipping {}: {why}", file.display())),
                Err(Skipped::Hidden | Skipped::NotShown) => {}
            }
        }
    }
    (without_copies(apps), problems)
}

/// `apps` with one of each set of entries that show the same name and run
/// the same command, like a package's entry and a copy the user's config
/// adds under another id: the one that opens file or link types, else the
/// first by id.
fn without_copies(apps: BTreeMap<String, Entry>) -> BTreeMap<String, Entry> {
    let mut kept: BTreeMap<(String, Vec<String>, bool), Entry> = BTreeMap::new();
    for entry in apps.into_values() {
        let key = (entry.name.clone(), entry.exec.clone(), entry.terminal);
        match kept.get(&key) {
            Some(other) if other.opens_types || !entry.opens_types => {}
            _ => {
                kept.insert(key, entry);
            }
        }
    }
    kept.into_values().map(|entry| (entry.id.clone(), entry)).collect()
}

fn read_entry(file: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(file)?.take(MAX_ENTRY_BYTES).read_to_string(&mut text)?;
    Ok(text)
}

/// The `.desktop` files under `dir`, each with its desktop file id: its
/// path below `root` with `/` turned into `-`. In name order, so a folder
/// read twice gives the same ids the same files.
fn entry_files(root: &Path, dir: &Path, depth: usize, found: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        // Followed through links: Nix's folders are all links.
        if path.is_dir() {
            if depth < MAX_DEPTH {
                entry_files(root, &path, depth + 1, found);
            }
        } else if path.extension().is_some_and(|extension| extension == ENTRY_EXTENSION)
            && let Ok(relative) = path.strip_prefix(root)
        {
            let id = relative.to_string_lossy().replace('/', "-");
            found.push((id, path));
        }
    }
}

/// What to watch so a change to the apps is seen, and the paths whose
/// changes count.
#[derive(Debug, Default, PartialEq)]
pub struct WatchPoints {
    /// Folders whose files and subfolders are apps (`applications/` and
    /// below, where they resolve to).
    pub trees: BTreeSet<PathBuf>,
    /// Folders holding a link on the way to an `applications/` folder, or the
    /// data folder it may appear in. Only `entries` count there.
    pub parents: BTreeSet<PathBuf>,
    /// Those links and would-be folders.
    pub entries: BTreeSet<PathBuf>,
}

impl WatchPoints {
    /// Whether a change at `path` may change the apps.
    pub fn counts(&self, path: &Path) -> bool {
        self.entries.contains(path) || self.trees.iter().any(|tree| path.starts_with(tree))
    }
}

/// Where to watch for `data_dirs`' apps. A NixOS rebuild or a Nix profile
/// switch swaps a link far above `applications/` (`/run/current-system`,
/// `~/.local/state/nix/profiles/profile`) and leaves the old folders as
/// they were, so each link on the way there is watched in its folder.
pub fn watch_points(data_dirs: &[PathBuf]) -> WatchPoints {
    let mut points = WatchPoints::default();
    for dir in data_dirs {
        let applications = dir.join("applications");
        links_on_the_way(&applications, &mut points);
        match std::fs::canonicalize(&applications) {
            // Watched through its folder too, so a replacement (one renamed
            // over it, or one made after it was deleted) is seen.
            Ok(resolved) => {
                if let Some(parent) = resolved.parent() {
                    points.parents.insert(parent.to_owned());
                    points.entries.insert(resolved.clone());
                }
                linked_folders(&resolved, 0, &mut points);
                points.trees.insert(resolved);
            }
            // Not there (yet): the folder it would appear in is watched.
            Err(_) => {
                if let Some(existing) = applications.ancestors().skip(1).find(|dir| dir.is_dir()) {
                    points.parents.insert(existing.to_owned());
                    let child = applications.strip_prefix(existing).ok().and_then(|rest| rest.components().next());
                    points.entries.extend(child.map(|child| existing.join(child)));
                }
            }
        }
    }
    points
}

/// Adds the folders that links below `dir` lead to as trees of their own:
/// a tree's watches don't follow links, though reading it does.
fn linked_folders(dir: &Path, depth: usize, points: &mut WatchPoints) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for path in entries.flatten().map(|entry| entry.path()) {
        if depth >= MAX_DEPTH || !path.is_dir() {
            continue;
        }
        match path.is_symlink() {
            true => {
                if let Ok(target) = std::fs::canonicalize(&path)
                    && points.trees.insert(target.clone())
                {
                    linked_folders(&target, depth + 1, points);
                }
            }
            false => linked_folders(&path, depth + 1, points),
        }
    }
}

/// Adds each link on the way to `path`, following them, with its folder.
fn links_on_the_way(path: &Path, points: &mut WatchPoints) {
    let mut path = path.to_owned();
    'resolve: for _ in 0..MAX_LINKS {
        let mut prefix = PathBuf::new();
        let mut components = path.components();
        while let Some(component) = components.next() {
            prefix.push(component);
            let Ok(target) = std::fs::read_link(&prefix) else { continue };
            let Some(parent) = prefix.parent() else { return };
            points.parents.insert(parent.to_owned());
            points.entries.insert(prefix.clone());
            path = parent.join(target).join(components.as_path());
            continue 'resolve;
        }
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launcher::desktop_entry::Locale;
    use std::os::unix::fs::symlink;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("novad-scan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_app(dir: &Path, relative: &str, name: &str, extra: &str) {
        let path = dir.join("applications").join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("[Desktop Entry]\nType=Application\nName={name}\nExec=true\n{extra}")).unwrap();
    }

    fn names(dirs: &[PathBuf]) -> BTreeMap<String, String> {
        let locale = Locale::default();
        let context = Context { locale: &locale, desktops: &[], path_var: "" };
        scan(dirs, &context).0.into_iter().map(|(id, entry)| (id, entry.name)).collect()
    }

    #[test]
    fn the_first_folder_wins_and_hidden_hides_the_rest() {
        let root = temp("order");
        let (user, system) = (root.join("user"), root.join("system"));
        write_app(&system, "foot.desktop", "Foot (system)", "");
        write_app(&user, "foot.desktop", "Foot (mine)", "");
        write_app(&system, "kde/dolphin.desktop", "Dolphin", "");
        write_app(&system, "firefox.desktop", "Firefox", "");
        write_app(&user, "firefox.desktop", "Firefox", "Hidden=true\n");
        write_app(&system, "not-an-app.txt", "Text", "");
        let found = names(&[user, system]);
        assert_eq!(found.get("foot.desktop").map(String::as_str), Some("Foot (mine)"));
        assert_eq!(found.get("kde-dolphin.desktop").map(String::as_str), Some("Dolphin"));
        assert!(!found.contains_key("firefox.desktop"), "the user deleted it");
        assert_eq!(found.len(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_copy_of_an_app_under_another_id_shows_once() {
        // Like a home-manager entry beside the package's own.
        let root = temp("copies");
        let share = root.join("share");
        write_app(&share, "obsidian.desktop", "Obsidian", "StartupWMClass=obsidian\n");
        let package =
            "[Desktop Entry]\nType=Application\nName=Obsidian\nExec=true %u\nMimeType=x-scheme-handler/obsidian;\n";
        std::fs::write(share.join("applications/md.obsidian.Obsidian.desktop"), package).unwrap();
        write_app(&share, "other.desktop", "Other", "");
        let found = names(std::slice::from_ref(&share));
        assert_eq!(found.keys().collect::<Vec<_>>(), ["md.obsidian.Obsidian.desktop", "other.desktop"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn apps_behind_links_are_found_and_the_links_watched() {
        // Like NixOS: /run/current-system -> a generation, its share -> the store.
        let root = temp("links");
        let generation = root.join("store/system-1");
        write_app(&generation.join("sw/share"), "foot.desktop", "Foot", "");
        let run = root.join("run");
        std::fs::create_dir_all(&run).unwrap();
        symlink(&generation, run.join("current-system")).unwrap();
        let data_dir = run.join("current-system/sw/share");
        assert_eq!(names(std::slice::from_ref(&data_dir)).len(), 1);

        let points = watch_points(std::slice::from_ref(&data_dir));
        assert!(points.parents.contains(&run));
        assert!(points.counts(&run.join("current-system")), "a rebuild swaps this link");
        assert!(points.trees.contains(&std::fs::canonicalize(generation.join("sw/share/applications")).unwrap()));
        assert!(!points.counts(&run.join("unrelated.pid")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_missing_applications_folder_is_watched_for() {
        let root = temp("missing");
        let points = watch_points(&[root.join("share")]);
        assert!(points.parents.contains(&root));
        assert!(points.counts(&root.join("share")));
        std::fs::remove_dir_all(root).unwrap();
    }
}
