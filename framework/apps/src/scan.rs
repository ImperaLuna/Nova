use super::desktop_entry::{self, Context, Entry, Skipped};
use nova_watch::WatchPoints;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const ENTRY_EXTENSION: &str = "desktop";
const MAX_DEPTH: usize = 8;
const MAX_ENTRY_BYTES: u64 = 1024 * 1024;

pub struct Found {
    pub apps: BTreeMap<String, Entry>,
    pub unlisted: BTreeMap<String, Entry>,
    pub problems: Vec<String>,
}

pub fn scan(data_dirs: &[PathBuf], context: &Context) -> Found {
    let mut apps = BTreeMap::new();
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
    let (apps, unlisted): (BTreeMap<String, Entry>, BTreeMap<String, Entry>) =
        apps.into_iter().partition(|(_, entry)| entry.listed);
    let mut unlisted: BTreeMap<String, Entry> = unlisted.into_iter().filter(|(_, entry)| entry.opens_types()).collect();
    let (apps, copies) = without_copies(apps);
    unlisted.extend(copies.into_iter().map(|entry| (entry.id.clone(), entry)));
    Found { apps, unlisted, problems }
}

fn without_copies(apps: BTreeMap<String, Entry>) -> (BTreeMap<String, Entry>, Vec<Entry>) {
    let mut kept: BTreeMap<(String, Vec<String>, bool), Entry> = BTreeMap::new();
    let mut copies = Vec::new();
    for entry in apps.into_values() {
        let key = (entry.name.clone(), entry.exec.clone(), entry.terminal);
        match kept.get(&key) {
            Some(other) if other.opens_types() || !entry.opens_types() => copies.push(entry),
            _ => copies.extend(kept.insert(key, entry)),
        }
    }
    (kept.into_values().map(|entry| (entry.id.clone(), entry)).collect(), copies)
}

pub fn read_entry(file: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(file)?.take(MAX_ENTRY_BYTES).read_to_string(&mut text)?;
    Ok(text)
}

fn entry_files(root: &Path, dir: &Path, depth: usize, found: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
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

const USER_ICON_FOLDERS: [&str; 2] = ["icons/hicolor", "pixmaps"];

pub fn watch_points(data_dirs: &[PathBuf]) -> WatchPoints {
    let mut points = WatchPoints::default();
    for dir in data_dirs {
        points.add_tree(&dir.join("applications"), MAX_DEPTH);
    }
    for folder in data_dirs.first().into_iter().flat_map(|dir| USER_ICON_FOLDERS.map(|folder| dir.join(folder))) {
        points.add_tree(&folder, MAX_DEPTH);
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_entry::Locale;
    use std::os::unix::fs::symlink;

    fn temp(name: &str) -> nova_test_support::TempDir {
        nova_test_support::TempDir::new(&format!("scan-{name}"))
    }

    fn write_app(dir: &Path, relative: &str, name: &str, extra: &str) {
        let path = dir.join("applications").join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("[Desktop Entry]\nType=Application\nName={name}\nExec=true\n{extra}")).unwrap();
    }

    fn names(dirs: &[PathBuf]) -> BTreeMap<String, String> {
        let locale = Locale::default();
        let context = Context { locale: &locale, desktops: &[], path_var: "", autostart: false };
        scan(dirs, &context).apps.into_iter().map(|(id, entry)| (id, entry.name)).collect()
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
    }

    #[test]
    fn unlisted_apps_are_kept_only_when_they_open_files() {
        let root = temp("unlisted");
        let share = root.join("share");
        write_app(&share, "viewer.desktop", "Viewer", "NoDisplay=true\nMimeType=image/png;\n");
        write_app(&share, "helper.desktop", "Helper", "NoDisplay=true\n");
        let locale = Locale::default();
        let context = Context { locale: &locale, desktops: &[], path_var: "", autostart: false };
        let found = scan(std::slice::from_ref(&share), &context);
        assert!(found.apps.is_empty());
        assert_eq!(found.unlisted.keys().collect::<Vec<_>>(), ["viewer.desktop"]);
    }

    #[test]
    fn a_copy_of_an_app_under_another_id_shows_once() {
        let root = temp("copies");
        let share = root.join("share");
        write_app(&share, "obsidian.desktop", "Obsidian", "StartupWMClass=obsidian\n");
        let package =
            "[Desktop Entry]\nType=Application\nName=Obsidian\nExec=true %u\nMimeType=x-scheme-handler/obsidian;\n";
        std::fs::write(share.join("applications/md.obsidian.Obsidian.desktop"), package).unwrap();
        write_app(&share, "other.desktop", "Other", "");
        let found = names(std::slice::from_ref(&share));
        assert_eq!(found.keys().collect::<Vec<_>>(), ["md.obsidian.Obsidian.desktop", "other.desktop"]);
        let locale = Locale::default();
        let context = Context { locale: &locale, desktops: &[], path_var: "", autostart: false };
        let unlisted = scan(std::slice::from_ref(&share), &context).unlisted;
        assert_eq!(unlisted.keys().collect::<Vec<_>>(), ["obsidian.desktop"]);
    }

    #[test]
    fn apps_behind_links_are_found_and_the_links_watched() {
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
    }

    #[test]
    fn a_missing_applications_folder_is_watched_for() {
        let root = temp("missing");
        let points = watch_points(&[root.join("share")]);
        assert!(points.parents.contains(root.path()));
        assert!(points.counts(&root.join("share")));
    }
}
