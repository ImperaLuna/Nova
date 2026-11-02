//! Which apps open which types of file, as the freedesktop.org "MIME
//! Applications Associations" spec 1.0.1 says: the `mimeapps.list` files
//! (the user's config, the system's, then the deprecated data folder ones,
//! each with a desktop-specific `<desktop>-mimeapps.list` first) and the
//! types each app's entry says it opens (`MimeType=`).
//!
//! A type's default is the first installed app a `[Default Applications]`
//! line names for it or, failing that, for a type it's a kind of; with
//! none named, the first app that opens it. The apps that open a type are
//! those `[Added Associations]` add, then those whose entries list it,
//! less those `[Removed Associations]` takes away in the same file or one
//! with more say; a type's parents add theirs after its own.
//!
//! Setting a default writes the user's own file, keeping everything else
//! in it. A `mimeapps.list` that's a link (home-manager's, into the Nix
//! store) is left alone and the desktop's own file written instead, which
//! has more say.

use super::desktop_entry::Entry;
use super::index::State;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "mimeapps.list";
const DEFAULTS: &str = "Default Applications";
const ADDED: &str = "Added Associations";
const REMOVED: &str = "Removed Associations";
/// The spec's default for an unset `XDG_CONFIG_DIRS`.
const DEFAULT_CONFIG_DIRS: &str = "/etc/xdg";
/// A `mimeapps.list` is a few kilobytes; anything far bigger isn't one.
const MAX_LIST_BYTES: u64 = 1024 * 1024;

/// Where the lists are, the one with most say first, and where the
/// user's own are.
#[derive(Debug, Clone)]
pub struct Places {
    /// Every list, the one with most say first.
    pub lists: Vec<PathBuf>,
    /// The user's own lists that a default may be written to: the
    /// desktops' own first, then `mimeapps.list`.
    pub user: Vec<PathBuf>,
}

impl Places {
    /// This session's: `config_home`, then `XDG_CONFIG_DIRS`, then each of
    /// `data_dirs`' `applications/` (the first is the user's).
    pub fn system(data_dirs: &[PathBuf]) -> Self {
        let config_dirs = nova_paths::path_var("XDG_CONFIG_DIRS").unwrap_or_else(|| DEFAULT_CONFIG_DIRS.into());
        let config_dirs: Vec<PathBuf> = std::env::split_paths(&config_dirs).collect();
        Self::new(nova_paths::config_home(), &config_dirs, data_dirs, &super::current_desktops())
    }

    pub fn new(config_home: Option<PathBuf>, config_dirs: &[PathBuf], data_dirs: &[PathBuf], desktops: &[String]) -> Self {
        let names: Vec<String> = desktops
            .iter()
            .map(|desktop| format!("{}-{FILE_NAME}", desktop.to_lowercase()))
            .chain([FILE_NAME.to_string()])
            .collect();
        let in_dir = |dir: &Path| names.iter().map(|name| dir.join(name)).collect::<Vec<_>>();
        let user = config_home.as_deref().map(in_dir).unwrap_or_default();
        let mut lists = user.clone();
        for dir in config_dirs.iter().filter(|dir| dir.is_absolute()) {
            lists.extend(in_dir(dir));
        }
        for dir in data_dirs {
            lists.extend(in_dir(&dir.join("applications")));
        }
        Self { lists, user }
    }
}

/// One list's three groups: type to app ids.
#[derive(Debug, Default)]
struct List {
    defaults: HashMap<String, Vec<String>>,
    added: HashMap<String, Vec<String>>,
    removed: HashMap<String, Vec<String>>,
}

/// Every list, as last read.
#[derive(Debug, Default)]
pub struct Associations {
    lists: Vec<List>,
}

impl Associations {
    /// Reads the lists that are there.
    pub fn load(places: &Places) -> Self {
        Self { lists: places.lists.iter().filter_map(|path| read(path).ok().flatten()).map(|text| parse(&text)).collect() }
    }

    /// The app that opens a file of `types` (a type, then the types it's a
    /// kind of, the nearest first) by default.
    pub fn default_app<'a>(&self, types: &[String], apps: &'a State) -> Option<&'a Entry> {
        let named = types.iter().find_map(|mime| {
            self.lists.iter().find_map(|list| list.defaults.get(mime)?.iter().find_map(|id| apps.any_entry(id)))
        });
        named.or_else(|| self.apps_for(types, apps).into_iter().next())
    }

    /// Every app that opens a file of `types`, its own type's first.
    pub fn apps_for<'a>(&self, types: &[String], apps: &'a State) -> Vec<&'a Entry> {
        let mut found: Vec<&Entry> = Vec::new();
        for mime in types {
            let mut removed = BTreeSet::new();
            for list in &self.lists {
                for id in list.added.get(mime).into_iter().flatten() {
                    if !removed.contains(id)
                        && let Some(entry) = apps.any_entry(id)
                        && !found.iter().any(|known| known.id == entry.id)
                    {
                        found.push(entry);
                    }
                }
                removed.extend(list.removed.get(mime).into_iter().flatten());
            }
            for entry in apps.openers() {
                if entry.mime_types.iter().any(|opens| opens == mime)
                    && !removed.contains(&entry.id)
                    && !found.iter().any(|known| known.id == entry.id)
                {
                    found.push(entry);
                }
            }
        }
        found
    }
}

/// Makes `app` the default for `mime` in the user's list that has the most
/// say and can be written: one that's a plain file, else `mimeapps.list`
/// when there's none, else (it's a link someone else keeps) the desktop's
/// own. Returns the file written.
pub fn set_default(places: &Places, mime: &str, app: &str) -> Result<PathBuf, String> {
    let kind = |path: &Path| std::fs::symlink_metadata(path).ok().map(|metadata| metadata.file_type());
    let plain = places.user.iter().find(|path| kind(path).is_some_and(|kind| kind.is_file()));
    let target = match plain {
        Some(path) => path.clone(),
        None => {
            let (main, desktops) = places.user.split_last().ok_or("there's no config folder to write to")?;
            match (kind(main), desktops.first()) {
                (None, _) => main.clone(),
                (Some(_), Some(desktop)) => desktop.clone(),
                (Some(_), None) => return Err(format!("{} is kept by something else", main.display())),
            }
        }
    };
    let text = read(&target).map_err(|e| format!("can't read {}: {e}", target.display()))?.unwrap_or_default();
    crate::state_file::write(&target, with_default(&text, mime, app).as_bytes())
        .map_err(|e| format!("can't write {}: {e}", target.display()))?;
    Ok(target)
}

/// `text`, a list, with `app` the default for `mime` and first among the
/// apps added for it, and no longer removed from it.
fn with_default(text: &str, mime: &str, app: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    set_key(&mut lines, DEFAULTS, mime, |_| vec![app.to_string()]);
    set_key(&mut lines, ADDED, mime, |ids| [app.to_string()].into_iter().chain(ids.into_iter().filter(|id| id != app)).collect());
    let mut removed = None;
    if let Some(line) = find_key(&lines, REMOVED, mime) {
        removed = Some((line, ids(lines[line].split_once('=').map_or("", |(_, value)| value))));
    }
    if let Some((line, ids)) = removed {
        let kept: Vec<String> = ids.into_iter().filter(|id| id != app).collect();
        if kept.is_empty() {
            lines.remove(line);
        } else {
            lines[line] = format!("{mime}={};", kept.join(";"));
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Sets `key` in `group` to what `value` makes of its ids now, adding the
/// group at the end when there's none.
fn set_key(lines: &mut Vec<String>, group: &str, key: &str, value: impl FnOnce(Vec<String>) -> Vec<String>) {
    let line = |ids: Vec<String>| format!("{key}={};", ids.join(";"));
    if let Some(at) = find_key(lines, group, key) {
        let now = ids(lines[at].split_once('=').map_or("", |(_, value)| value));
        lines[at] = line(value(now));
        return;
    }
    let header = format!("[{group}]");
    match lines.iter().position(|existing| existing.trim() == header) {
        Some(at) => lines.insert(at + 1, line(value(vec![]))),
        None => {
            if lines.last().is_some_and(|last| !last.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(header);
            lines.push(line(value(vec![])));
        }
    }
}

/// The line of `key` in `group`.
fn find_key(lines: &[String], group: &str, key: &str) -> Option<usize> {
    let header = format!("[{group}]");
    let mut inside = false;
    for (at, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == header;
        } else if inside && line.split_once('=').is_some_and(|(name, _)| name.trim() == key) {
            return Some(at);
        }
    }
    None
}

/// A list's text: its groups' keys, the first of a repeated key winning.
fn parse(text: &str) -> List {
    let mut list = List::default();
    let mut group = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|line| line.strip_suffix(']')) {
            group = match name {
                DEFAULTS => Some(&mut list.defaults),
                ADDED => Some(&mut list.added),
                REMOVED => Some(&mut list.removed),
                _ => None,
            };
            continue;
        }
        if let (Some(group), Some((mime, value))) = (group.as_deref_mut(), line.split_once('=')) {
            group.entry(mime.trim().to_string()).or_insert_with(|| ids(value));
        }
    }
    list
}

/// `a.desktop;b.desktop;`'s ids.
fn ids(value: &str) -> Vec<String> {
    value.split(';').map(str::trim).filter(|id| !id.is_empty()).map(str::to_string).collect()
}

/// A list's text, `None` when there's no such file.
fn read(path: &Path) -> std::io::Result<Option<String>> {
    use std::io::Read;
    let mut text = String::new();
    match std::fs::File::open(path) {
        Ok(file) => file.take(MAX_LIST_BYTES).read_to_string(&mut text).map(|_| Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    #[test]
    fn lists_are_found_in_the_specs_order() {
        let places = Places::new(
            Some("/home/me/.config".into()),
            &["/etc/xdg".into()],
            &["/home/me/.local/share".into(), "/usr/share".into()],
            &["niri".into()],
        );
        let lists: Vec<&str> = places.lists.iter().map(|path| path.to_str().unwrap()).collect();
        assert_eq!(
            lists,
            [
                "/home/me/.config/niri-mimeapps.list",
                "/home/me/.config/mimeapps.list",
                "/etc/xdg/niri-mimeapps.list",
                "/etc/xdg/mimeapps.list",
                "/home/me/.local/share/applications/niri-mimeapps.list",
                "/home/me/.local/share/applications/mimeapps.list",
                "/usr/share/applications/niri-mimeapps.list",
                "/usr/share/applications/mimeapps.list",
            ]
        );
        assert_eq!(places.user, [PathBuf::from("/home/me/.config/niri-mimeapps.list"), "/home/me/.config/mimeapps.list".into()]);
    }

    #[test]
    fn setting_a_default_keeps_the_rest_of_the_list() {
        let text = "# mine\n[Added Associations]\nimage/png=gimp.desktop;loupe.desktop;\n\n[Default Applications]\n\
                    text/plain=gedit.desktop;\nimage/png=gimp.desktop;\n\n[Removed Associations]\nimage/png=loupe.desktop;feh.desktop;\n";
        let out = with_default(text, "image/png", "loupe.desktop");
        let list = parse(&out);
        assert_eq!(list.defaults["image/png"], ["loupe.desktop"]);
        assert_eq!(list.defaults["text/plain"], ["gedit.desktop"]);
        assert_eq!(list.added["image/png"], ["loupe.desktop", "gimp.desktop"]);
        assert_eq!(list.removed["image/png"], ["feh.desktop"]);
        assert!(out.starts_with("# mine\n"));

        let fresh = parse(&with_default("", "application/pdf", "zathura.desktop"));
        assert_eq!(fresh.defaults["application/pdf"], ["zathura.desktop"]);
        assert_eq!(fresh.added["application/pdf"], ["zathura.desktop"]);
    }

    #[test]
    fn a_default_goes_to_a_plain_file_never_over_a_link() {
        let dir = TempDir::new("mimeapps-write");
        let config = dir.join("config");
        std::fs::create_dir_all(&config).unwrap();
        let places = Places::new(Some(config.clone()), &[], &[], &["niri".into()]);
        // No list yet: mimeapps.list.
        assert_eq!(set_default(&places, "image/png", "a.desktop").unwrap(), config.join(FILE_NAME));
        // home-manager's link into the store: the desktop's own list.
        std::fs::remove_file(config.join(FILE_NAME)).unwrap();
        let store = dir.join("store-mimeapps.list");
        std::fs::write(&store, "[Default Applications]\nimage/png=b.desktop;\n").unwrap();
        std::os::unix::fs::symlink(&store, config.join(FILE_NAME)).unwrap();
        assert_eq!(set_default(&places, "image/png", "a.desktop").unwrap(), config.join("niri-mimeapps.list"));
        assert!(std::fs::symlink_metadata(config.join(FILE_NAME)).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&store).unwrap(), "[Default Applications]\nimage/png=b.desktop;\n");
        // Now that one is a plain file, it's written again.
        assert_eq!(set_default(&places, "text/plain", "c.desktop").unwrap(), config.join("niri-mimeapps.list"));
        let list = parse(&std::fs::read_to_string(config.join("niri-mimeapps.list")).unwrap());
        assert_eq!(list.defaults["image/png"], ["a.desktop"]);
        assert_eq!(list.defaults["text/plain"], ["c.desktop"]);
    }
}
