//! Steam's installed games: every library its `libraryfolders.vdf` names,
//! on any drive, and in each the `appmanifest_<appid>.acf` files Steam
//! keeps for what it installed there (name, install folder, size).
//!
//! A game's folder is `steamapps/common/<installdir>` of its library;
//! Proton, the Steam Linux Runtime and other tools install there too and
//! are told apart by the `toolmanifest.vdf` they carry, so they aren't
//! listed. Its saves are looked for in its Proton prefix (the Windows
//! user's folder, where Documents, Saved Games and AppData are), else in
//! the Steam Cloud folder `userdata/<user>/<appid>` of the newest user.
//!
//! Every name and folder comes from Steam's files, so an app id must be
//! digits and an install folder one plain name: nothing reaches outside
//! the library.

use super::vdf;
use crate::logger::Logger;
use nova_protocol::{Game, wire_text};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

const LOG_SOURCE: &str = "files";
/// Where Steam keeps a library's manifests and games.
const STEAMAPPS: &str = "steamapps";
/// Steam's list of libraries, in its root's `steamapps`, or in `config`
/// for an older Steam.
const LIBRARY_LISTS: [&str; 2] = ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"];
/// What a compatibility tool (Proton, the Steam Linux Runtime) carries in
/// its folder, which no game does.
const TOOL_MANIFEST: &str = "toolmanifest.vdf";
/// "Steamworks Common Redistributables": installers Steam runs for games,
/// listed like a game but carrying no tool manifest.
const REDISTRIBUTABLES_APP: &str = "228980";
/// A game's Windows user folder inside its Proton prefix.
const PROTON_USER: &str = "pfx/drive_c/users/steamuser";
/// More libraries than anyone has; past it, the rest are skipped.
const MAX_LIBRARIES: usize = 64;
/// More manifests in one library than anyone has; past it, the rest are
/// skipped.
const MAX_MANIFESTS: usize = 4096;

/// A library and the Steam it belongs to (where `userdata` is).
struct Library {
    root: PathBuf,
    path: PathBuf,
}

/// The games in every library of the Steam installs at `roots`, each once.
pub fn games(roots: &[PathBuf], logger: &Logger) -> Vec<Game> {
    let mut seen_apps = HashSet::new();
    libraries(roots, logger)
        .iter()
        .flat_map(|library| manifests(&library.path, logger).into_iter().map(move |manifest| (library, manifest)))
        .filter_map(|(library, manifest)| game(library, &manifest, logger))
        .filter(|(app, _)| seen_apps.insert(app.clone()))
        .map(|(_, game)| game)
        .collect()
}

/// Every library of the Steam installs at `roots` that exist, each once.
fn libraries(roots: &[PathBuf], logger: &Logger) -> Vec<Library> {
    let (mut roots_seen, mut paths_seen) = (HashSet::new(), HashSet::new());
    let mut libraries = Vec::new();
    for root in roots.iter().filter_map(|root| root.canonicalize().ok()) {
        if !roots_seen.insert(root.clone()) {
            continue;
        }
        let mut paths = vec![root.clone()];
        if let Some(list) = LIBRARY_LISTS.iter().map(|list| root.join(list)).find(|list| list.is_file()) {
            match vdf::read(&list) {
                Ok(parsed) => paths.extend(library_paths(&parsed)),
                Err(e) => logger.warn(LOG_SOURCE, &format!("{}: {e}", list.display())),
            }
        }
        for path in paths.iter().filter_map(|path| path.canonicalize().ok()) {
            if libraries.len() < MAX_LIBRARIES && paths_seen.insert(path.clone()) {
                libraries.push(Library { root: root.clone(), path });
            }
        }
    }
    libraries
}

/// The absolute library paths `libraryfolders.vdf` lists: each entry's
/// `path`, or, from an older Steam, the entry itself.
fn library_paths(parsed: &vdf::Block) -> Vec<PathBuf> {
    let Some(entries) = vdf::block(parsed, "libraryfolders") else { return Vec::new() };
    entries
        .iter()
        .filter(|(key, _)| key.bytes().all(|byte| byte.is_ascii_digit()))
        .filter_map(|(_, value)| match value {
            vdf::Value::Block(entry) => vdf::text(entry, "path"),
            vdf::Value::Text(path) => Some(path.as_str()),
        })
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.components().all(|part| !matches!(part, Component::ParentDir)))
        .collect()
}

/// The app manifests in `library`, parsed, with their app id.
fn manifests(library: &Path, logger: &Logger) -> Vec<(String, vdf::Block)> {
    let Ok(entries) = std::fs::read_dir(library.join(STEAMAPPS)) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let app = name.strip_prefix("appmanifest_")?.strip_suffix(".acf")?.to_owned();
            app_id(&app).then_some((app, entry.path()))
        })
        .take(MAX_MANIFESTS)
        .filter_map(|(app, path)| match vdf::read(&path) {
            Ok(parsed) => Some((app, parsed)),
            Err(e) => {
                logger.warn(LOG_SOURCE, &format!("{}: {e}", path.display()));
                None
            }
        })
        .collect()
}

/// The game `manifest` describes in `library`, with its app id; none for a
/// tool, or one whose folder isn't there.
fn game(library: &Library, (app, manifest): &(String, vdf::Block), logger: &Logger) -> Option<(String, Game)> {
    let state = vdf::block(manifest, "AppState")?;
    let folder_name = vdf::text(state, "installdir")?;
    if !plain_name(folder_name) {
        logger.warn(LOG_SOURCE, &format!("app {app}: install folder {folder_name:?} isn't a plain name"));
        return None;
    }
    let folder = library.path.join(STEAMAPPS).join("common").join(folder_name);
    if app == REDISTRIBUTABLES_APP || !folder.is_dir() || folder.join(TOOL_MANIFEST).exists() {
        return None;
    }
    let name = vdf::text(state, "name").filter(|name| !name.trim().is_empty()).unwrap_or(folder_name);
    let size = vdf::text(state, "SizeOnDisk").and_then(|size| size.parse().ok()).filter(|size| *size > 0);
    let saves = saves(library, app).map(wire_text);
    Some((app.clone(), Game { name: name.trim().to_owned(), folder: wire_text(&folder), size, saves }))
}

/// Where game `app` of `library` keeps its saves: its Proton prefix's user
/// folder, else its Steam Cloud folder of the user who last played.
fn saves(library: &Library, app: &str) -> Option<PathBuf> {
    let proton = library.path.join(STEAMAPPS).join("compatdata").join(app).join(PROTON_USER);
    if proton.is_dir() {
        return Some(proton);
    }
    std::fs::read_dir(library.root.join("userdata"))
        .ok()?
        .filter_map(Result::ok)
        .map(|user| user.path().join(app))
        .filter_map(|cloud| Some((cloud.metadata().ok().filter(std::fs::Metadata::is_dir)?.modified().ok()?, cloud)))
        .max_by_key(|(modified, _): &(SystemTime, PathBuf)| *modified)
        .map(|(_, cloud)| cloud)
}

/// A Steam app id: digits.
fn app_id(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// One name in a folder: no `/`, not `.` or `..`.
fn plain_name(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!((parts.next(), parts.next()), (Some(Component::Normal(_)), None)) && !name.contains('/')
}
