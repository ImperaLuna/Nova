//! The file pickers apps are waiting on (`portal.rs` asks, the Nova Files
//! window answers with `files_pick` or `files_pick_cancel`): each one's
//! question, its answer slot, and the checks an answer must pass before
//! the app gets it. An answer that doesn't fit (a folder for a file, a file
//! to open that isn't there) is refused and the picker keeps waiting.
//!
//! Each app's last picked folder is kept while novad runs, so its next
//! picker opens there, as Explorer does.

use super::target;
use crate::lock;
use crate::reply_slot::Slot;
use nova_protocol::{FILES_PICK_EXISTS, FilePicker, PickMode};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Most pickers waiting at once, across apps; more are turned down.
const MAX_OPEN: usize = 16;
/// Most apps whose last folder is kept.
const MAX_REMEMBERED: usize = 64;

/// How a picker ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The paths to give the app, checked, and the filter shown.
    Picked { paths: Vec<PathBuf>, filter: Option<u32> },
    /// Closed without picking.
    Cancelled,
    /// Ended without the user: the app gave up, Nova Files went away.
    Ended,
}

pub type AnswerSlot = Slot<Answer>;

#[derive(Default)]
pub struct Pickers {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    next: u32,
    open: HashMap<u32, Asked>,
    /// Each app's last picked folder, by its app id.
    last_folders: HashMap<String, PathBuf>,
}

struct Asked {
    picker: FilePicker,
    app: String,
    /// The names `save_files` writes into the picked folder.
    files: Vec<OsString>,
    slot: Arc<AnswerSlot>,
}

impl Pickers {
    /// Waits on `picker` for the app `app`, numbering it. `files` are the
    /// names a `save_files` writes. `None` when [`MAX_OPEN`] already wait.
    pub fn ask(
        &self,
        app: &str,
        mut picker: FilePicker,
        files: Vec<OsString>,
    ) -> Option<(FilePicker, Arc<AnswerSlot>)> {
        let mut state = lock(&self.state);
        if state.open.len() >= MAX_OPEN {
            return None;
        }
        state.next = state.next.wrapping_add(1).max(1);
        picker.id = state.next;
        let slot = Arc::new(AnswerSlot::default());
        let asked = Asked { picker: picker.clone(), app: app.to_owned(), files, slot: Arc::clone(&slot) };
        state.open.insert(picker.id, asked);
        Some((picker, slot))
    }

    /// The folder the app `app` last picked in, while it's still a folder.
    pub fn last_folder(&self, app: &str) -> Option<PathBuf> {
        lock(&self.state).last_folders.get(app).filter(|folder| folder.is_dir()).cloned()
    }

    /// Answers picker `id` with `paths` (wire text) once they fit it, and
    /// the filter `filter` shown; a save over a file only when `replace`.
    pub fn pick(&self, id: u32, paths: &[String], filter: Option<u32>, replace: bool) -> Result<(), String> {
        let mut state = lock(&self.state);
        let asked = state.open.get(&id).ok_or_else(|| format!("no file picker {id} is waiting"))?;
        if filter.is_some_and(|filter| filter as usize >= asked.picker.filters.len()) {
            return Err("no such filter".into());
        }
        let paths = checked(&asked.picker, &asked.files, paths, replace)?;
        let asked = state.open.remove(&id).expect("found above");
        // The folder holding what was picked (for save_files, the folder
        // picked: each path is a name in it).
        if let Some(folder) = paths.first().and_then(|path| path.parent()) {
            if state.last_folders.len() >= MAX_REMEMBERED && !state.last_folders.contains_key(&asked.app) {
                state.last_folders.clear();
            }
            state.last_folders.insert(asked.app, folder.to_path_buf());
        }
        asked.slot.resolve(Answer::Picked { paths, filter });
        Ok(())
    }

    /// Ends picker `id` with `answer`. False when it wasn't waiting.
    pub fn end(&self, id: u32, answer: Answer) -> bool {
        let asked = lock(&self.state).open.remove(&id);
        asked.map(|asked| asked.slot.resolve(answer)).is_some()
    }

    /// Ends every waiting picker but those `keep` names.
    pub fn end_all(&self, keep: impl Fn(u32) -> bool) {
        let mut state = lock(&self.state);
        let ended: Vec<u32> = state.open.keys().copied().filter(|id| !keep(*id)).collect();
        for id in ended {
            if let Some(asked) = state.open.remove(&id) {
                asked.slot.resolve(Answer::Ended);
            }
        }
    }
}

/// The paths `paths` (wire text) a picker gives its app, once they fit it:
/// - `open`: one, or more when `multiple`; each there now, a folder when it
///   picks folders and a file otherwise.
/// - `save`: one file, in a folder that's there, not itself a folder, and
///   not there yet unless `replace` ([`FILES_PICK_EXISTS`]).
/// - `save_files`: one folder that's there; the app gets each of `files`
///   in it.
fn checked(picker: &FilePicker, files: &[OsString], paths: &[String], replace: bool) -> Result<Vec<PathBuf>, String> {
    let most = if picker.mode == PickMode::Open && picker.multiple { usize::MAX } else { 1 };
    if paths.is_empty() || paths.len() > most {
        return Err(if most == 1 { "pick one item".into() } else { "pick at least one item".into() });
    }
    let cleaned = paths.iter().map(|path| target::clean(path)).collect::<Result<Vec<_>, _>>()?;
    match picker.mode {
        PickMode::Open => {
            for path in &cleaned {
                let metadata = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
                match (picker.folders, metadata.is_dir()) {
                    (true, false) => return Err(format!("{} isn't a folder", path.display())),
                    (false, true) => return Err(format!("{} is a folder", path.display())),
                    _ => {}
                }
            }
            Ok(cleaned)
        }
        PickMode::Save => {
            let path = &cleaned[0];
            if path.file_name().is_none() || !path.parent().is_some_and(std::path::Path::is_dir) {
                return Err(format!("{} isn't a file in a folder", path.display()));
            }
            if path.is_dir() {
                return Err(format!("{} is a folder", path.display()));
            }
            if !replace && std::fs::symlink_metadata(path).is_ok() {
                return Err(FILES_PICK_EXISTS.into());
            }
            Ok(cleaned)
        }
        PickMode::SaveFiles => {
            let folder = &cleaned[0];
            if !folder.is_dir() {
                return Err(format!("{} isn't a folder", folder.display()));
            }
            Ok(files.iter().map(|name| folder.join(name)).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    fn picker(mode: PickMode, multiple: bool, folders: bool) -> FilePicker {
        FilePicker {
            id: 0,
            mode,
            title: String::new(),
            accept: String::new(),
            multiple,
            folders,
            name: String::new(),
            filters: vec![],
            filter: 0,
        }
    }

    fn dir() -> TempDir {
        let dir = TempDir::new("files-picker");
        std::fs::create_dir_all(dir.join("Music")).unwrap();
        std::fs::write(dir.join("Music/song.flac"), b"").unwrap();
        std::fs::write(dir.join("Music/other.flac"), b"").unwrap();
        dir
    }

    fn wire(dir: &TempDir, relative: &str) -> String {
        nova_protocol::wire_text(dir.join(relative))
    }

    #[test]
    fn open_takes_files_that_are_there_one_unless_multiple() {
        let dir = dir();
        let (song, other) = (wire(&dir, "Music/song.flac"), wire(&dir, "Music/other.flac"));
        let single = picker(PickMode::Open, false, false);
        assert_eq!(checked(&single, &[], std::slice::from_ref(&song), false).unwrap(), [dir.join("Music/song.flac")]);
        assert!(checked(&single, &[], &[song.clone(), other.clone()], false).is_err());
        assert!(checked(&single, &[], &[wire(&dir, "Music")], false).unwrap_err().contains("is a folder"));
        assert!(checked(&single, &[], &[wire(&dir, "Music/gone.flac")], false).is_err());
        assert!(checked(&single, &[], &[], false).is_err());
        assert!(checked(&single, &[], &["Music/song.flac".into()], false).unwrap_err().contains("absolute"));
        let several = picker(PickMode::Open, true, false);
        assert_eq!(checked(&several, &[], &[song.clone(), other], false).unwrap().len(), 2);
        let folders = picker(PickMode::Open, false, true);
        assert!(checked(&folders, &[], &[wire(&dir, "Music")], false).is_ok());
        assert!(checked(&folders, &[], &[song], false).unwrap_err().contains("isn't a folder"));
    }

    #[test]
    fn save_takes_a_new_or_existing_file_in_a_folder() {
        let dir = dir();
        let save = picker(PickMode::Save, false, false);
        assert!(checked(&save, &[], &[wire(&dir, "Music/new.flac")], false).is_ok());
        let song = [wire(&dir, "Music/song.flac")];
        assert_eq!(checked(&save, &[], &song, false).unwrap_err(), FILES_PICK_EXISTS);
        assert!(checked(&save, &[], &song, true).is_ok(), "replacing was asked about");
        assert!(checked(&save, &[], &[wire(&dir, "Music")], false).unwrap_err().contains("is a folder"));
        assert!(checked(&save, &[], &[wire(&dir, "Missing/new.flac")], false).is_err());
    }

    #[test]
    fn save_files_puts_the_apps_names_in_the_folder() {
        let dir = dir();
        let names = [OsString::from("a.txt"), OsString::from("b.txt")];
        let save = picker(PickMode::SaveFiles, false, false);
        assert_eq!(
            checked(&save, &names, &[wire(&dir, "Music")], false).unwrap(),
            [dir.join("Music/a.txt"), dir.join("Music/b.txt")]
        );
        assert!(checked(&save, &names, &[wire(&dir, "Music/song.flac")], false).is_err());
    }

    #[test]
    fn a_picked_folder_is_where_the_app_starts_next() {
        let dir = dir();
        let pickers = Pickers::default();
        let (asked, slot) = pickers.ask("org.mozilla.firefox", picker(PickMode::Open, false, false), vec![]).unwrap();
        assert!(pickers.pick(asked.id, &[wire(&dir, "Music")], None, false).is_err(), "a folder isn't a file");
        assert!(
            pickers.pick(asked.id, &[wire(&dir, "Music/song.flac")], Some(0), false).unwrap_err().contains("filter")
        );
        pickers.pick(asked.id, &[wire(&dir, "Music/song.flac")], None, false).unwrap();
        let answer = zbus::block_on(slot.wait());
        assert_eq!(answer, Answer::Picked { paths: vec![dir.join("Music/song.flac")], filter: None });
        assert_eq!(pickers.last_folder("org.mozilla.firefox"), Some(dir.join("Music")));
        assert_eq!(pickers.last_folder("code"), None);
        assert!(pickers.pick(asked.id, &[wire(&dir, "Music/song.flac")], None, false).is_err(), "answered once");
    }

    #[test]
    fn ending_answers_the_app_once() {
        let pickers = Pickers::default();
        let (first, first_slot) = pickers.ask("a", picker(PickMode::Save, false, false), vec![]).unwrap();
        let (second, second_slot) = pickers.ask("b", picker(PickMode::Save, false, false), vec![]).unwrap();
        assert_ne!(first.id, second.id);
        assert!(pickers.end(first.id, Answer::Cancelled));
        assert!(!pickers.end(first.id, Answer::Ended));
        assert_eq!(zbus::block_on(first_slot.wait()), Answer::Cancelled);
        pickers.end_all(|_| false);
        assert_eq!(zbus::block_on(second_slot.wait()), Answer::Ended);
    }
}
