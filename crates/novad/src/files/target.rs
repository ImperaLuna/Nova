//! What a Nova Files window shows, checked before it opens: the Home page
//! ([`FILES_HOME_PLACE`]) with nothing selected, or a folder that is an
//! absolute path with no `..` in it and a folder now, with every item to
//! select a path straight inside it. Paths come from the socket (as
//! wire text, `nova_protocol::path_to_wire`) and the session bus, so
//! nothing else is trusted about them; they go out as wire text too, so a
//! name that isn't UTF-8 opens like any other.
//!
//! Also turns the `file://` URIs apps give `org.freedesktop.FileManager1`
//! into windows: a folder to show, or items grouped by the folder holding
//! them, one window per folder.

use nova_protocol::{FILES_HOME_PLACE, FILES_PATH_MAX_BYTES, FilesWindow, wire_path, wire_text};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Component, PathBuf};

/// A window on `folder` with `select` selected, once both check out.
pub fn window(folder: &str, select: &[String]) -> Result<FilesWindow, String> {
    if folder == FILES_HOME_PLACE && select.is_empty() {
        return Ok(FilesWindow { folder: folder.into(), select: Vec::new() });
    }
    let folder = clean(folder)?;
    if !folder.is_dir() {
        return Err(format!("{} isn't a folder", folder.display()));
    }
    let select = select
        .iter()
        .map(|item| {
            let cleaned = clean(item)?;
            if cleaned.parent() != Some(folder.as_path()) {
                return Err(format!("{item} isn't in {}", folder.display()));
            }
            // Spelled as the folder's listing spells it.
            Ok(wire_text(&cleaned))
        })
        .collect::<Result<_, _>>()?;
    Ok(FilesWindow { folder: wire_text(&folder), select })
}

/// A window on each folder `uris` name (`ShowFolders`).
pub fn folders(uris: &[String]) -> Result<Vec<FilesWindow>, String> {
    uris.iter().map(|uri| window(&wire_text(&path(uri)?), &[])).collect()
}

/// A window on each folder holding items `uris` name, with them selected
/// (`ShowItems`), in the order the folders first come.
pub fn items(uris: &[String]) -> Result<Vec<FilesWindow>, String> {
    let mut windows: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for uri in uris {
        let item = path(uri)?;
        let folder = item.parent().ok_or_else(|| format!("{uri} has no folder"))?.to_path_buf();
        let item = wire_text(&item);
        match windows.iter_mut().find(|(known, _)| *known == folder) {
            Some((_, select)) => select.push(item),
            None => windows.push((folder, vec![item])),
        }
    }
    windows.iter().map(|(folder, select)| window(&wire_text(folder), select)).collect()
}

/// The path a `file://` URI names. Only local files: no other scheme, and
/// no host but `localhost`.
pub(super) fn path(uri: &str) -> Result<PathBuf, String> {
    let local = uri.strip_prefix("file://localhost/").map(|rest| format!("file:///{rest}"));
    let uri_or_local = local.as_deref().unwrap_or(uri);
    if !uri_or_local.starts_with("file:///") {
        return Err(format!("{uri} isn't a local file"));
    }
    // Each byte is at most a `%XX` escape.
    let escaped = uri_or_local.strip_prefix("file://").filter(|escaped| escaped.len() <= 3 * FILES_PATH_MAX_BYTES);
    let bytes = escaped.and_then(crate::notifications::image::percent_decoded);
    let path = bytes.map(|bytes| PathBuf::from(OsString::from_vec(bytes)));
    path.filter(|path| path.as_os_str().len() <= FILES_PATH_MAX_BYTES).ok_or_else(|| format!("{uri} isn't a file path"))
}

/// The path wire text `path` names, once it's absolute and has no `.` or
/// `..` in it.
pub(super) fn clean(path: &str) -> Result<PathBuf, String> {
    let path = wire_path(path);
    if !path.is_absolute() {
        return Err(format!("{} isn't an absolute path", path.display()));
    }
    if path.components().any(|component| matches!(component, Component::ParentDir | Component::CurDir)) {
        return Err(format!("{} has . or .. in it", path.display()));
    }
    // Without a trailing slash, so it matches an item's parent.
    Ok(path.components().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn dir(name: &str) -> nova_test_support::TempDir {
        let dir = nova_test_support::TempDir::new(&format!("files-target-{name}"));
        std::fs::create_dir_all(dir.join("Music/Albums")).unwrap();
        std::fs::write(dir.join("Music/song one.flac"), b"").unwrap();
        dir
    }

    fn display(path: &Path) -> String {
        path.display().to_string()
    }

    #[test]
    fn a_window_shows_a_folder_with_items_in_it_selected() {
        let dir = dir("window");
        let music = display(&dir.join("Music"));
        let song = display(&dir.join("Music/song one.flac"));
        let shown = window(&format!("{music}/"), &[format!("{music}//song one.flac")]).unwrap();
        assert_eq!(shown, FilesWindow { folder: music.clone(), select: vec![song.clone()] });
        assert_eq!(window(&music, &[]).unwrap().select, Vec::<String>::new());
        assert_eq!(window(FILES_HOME_PLACE, &[]).unwrap().folder, FILES_HOME_PLACE);
        assert!(window(FILES_HOME_PLACE, &[song]).is_err(), "nothing to select on the Home page");
    }

    #[test]
    fn bad_folders_and_items_are_refused() {
        let dir = dir("refused");
        let music = display(&dir.join("Music"));
        assert!(window("Music", &[]).unwrap_err().contains("absolute"));
        assert!(window(&format!("{music}/../Music"), &[]).unwrap_err().contains(".."));
        assert!(window(&display(&dir.join("Music/song one.flac")), &[]).unwrap_err().contains("isn't a folder"));
        assert!(window(&display(&dir.join("Missing")), &[]).unwrap_err().contains("isn't a folder"));
        let elsewhere = display(&dir.join("other.txt"));
        assert!(window(&music, &[elsewhere]).unwrap_err().contains("isn't in"));
        let deeper = display(&dir.join("Music/Albums/a.flac"));
        assert!(window(&music, &[deeper]).unwrap_err().contains("isn't in"), "only straight inside");
    }

    #[test]
    fn show_folders_opens_a_window_per_folder() {
        let dir = dir("folders");
        let uri = format!("file://{}", display(&dir.join("Music")));
        let shown = folders(&[uri.clone(), format!("{uri}/Albums")]).unwrap();
        let folders: Vec<_> = shown.iter().map(|window| window.folder.clone()).collect();
        assert_eq!(folders, [display(&dir.join("Music")), display(&dir.join("Music/Albums"))]);
    }

    #[test]
    fn show_items_groups_items_by_their_folder() {
        let dir = dir("items");
        let music = dir.join("Music");
        let song = format!("file://{}/song%20one.flac", display(&music));
        let album = format!("file://localhost{}/Albums", display(&music));
        let other = format!("file://{}", display(&dir.join("Music")));
        let shown = items(&[song, album, other]).unwrap();
        assert_eq!(shown.len(), 2);
        assert_eq!(shown[0].folder, display(&music));
        assert_eq!(shown[0].select, [display(&music.join("song one.flac")), display(&music.join("Albums"))]);
        assert_eq!(shown[1].folder, display(&dir));
        assert_eq!(shown[1].select, [display(&music)]);
    }

    /// "Show in folder" on a file whose name isn't UTF-8 selects it, by a
    /// name that comes back to the same file.
    #[test]
    fn an_item_named_in_latin1_is_shown() {
        use std::os::unix::ffi::OsStrExt;
        let dir = dir("latin1");
        let item = dir.join("Music").join(std::ffi::OsStr::from_bytes(b"caf\xe9.flac"));
        std::fs::write(&item, b"").unwrap();
        let uri = format!("file://{}/caf%E9.flac", display(&dir.join("Music")));
        let shown = items(&[uri]).unwrap();
        assert_eq!(shown[0].select, [wire_text(&item)]);
        assert_eq!(wire_path(&shown[0].select[0]), item);
    }

    #[test]
    fn only_local_file_uris_are_shown() {
        assert!(path("https://example.com/a").is_err());
        assert!(path("file://server/share/a").is_err());
        assert!(path("/tmp/a").is_err(), "a URI, not a path");
        assert!(path("file:///tmp/%zz").is_err());
        assert_eq!(path("file:///tmp/a%20b").unwrap(), PathBuf::from("/tmp/a b"));
    }
}
