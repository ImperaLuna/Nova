//! What a Nova Files window shows, checked before it opens: the folder is
//! an absolute path with no `..` in it that is a folder now, and every item
//! to select is a path straight inside it. Paths come from the socket and
//! the session bus, so nothing else is trusted about them.

use nova_protocol::FilesWindow;
use std::path::{Component, Path, PathBuf};

/// A window on `folder` with `select` selected, once both check out.
pub fn window(folder: &str, select: &[String]) -> Result<FilesWindow, String> {
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
            text(&cleaned)
        })
        .collect::<Result<_, _>>()?;
    Ok(FilesWindow { folder: text(&folder)?, select })
}

/// `path` as given, once it's absolute and has no `.` or `..` in it.
fn clean(path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(format!("{} isn't an absolute path", path.display()));
    }
    if path.components().any(|component| matches!(component, Component::ParentDir | Component::CurDir)) {
        return Err(format!("{} has . or .. in it", path.display()));
    }
    // Without a trailing slash, so it matches an item's parent.
    Ok(path.components().collect())
}

/// `path` as wire text: it must be UTF-8, which every path the UI can show is.
fn text(path: &Path) -> Result<String, String> {
    path.to_str().map(str::to_string).ok_or_else(|| format!("{} isn't UTF-8", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(shown, FilesWindow { folder: music.clone(), select: vec![song] });
        assert_eq!(window(&music, &[]).unwrap().select, Vec::<String>::new());
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
}
