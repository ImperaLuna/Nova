//! `nova files [path]...`: opens a Nova Files window on each path. A folder
//! opens itself, a file opens the folder holding it with the file selected,
//! and no path opens the home folder. It's what Nova Files' desktop entry
//! runs, so it's the default file manager's command too.
//!
//! Paths are made absolute here, against this command's folder, since
//! novad's is not the caller's; novad checks them again. Symlinks stay as
//! typed, like the shell's `cd`: a linked file is shown where its link is,
//! and a linked home folder still reads as home. They go out as wire text
//! (`nova_protocol::wire_text`), so any name opens, UTF-8 or not.

use crate::client::Client;
use nova_protocol::{Command, wire_text};
use std::path::{Component, Path, PathBuf};

#[derive(clap::Args)]
pub struct FilesArgs {
    /// Folders to open, or files to show in their folder. The home folder
    /// when none is given.
    paths: Vec<PathBuf>,
}

pub fn run(args: &FilesArgs, socket: &Path) -> Result<(), String> {
    let home = || nova_paths::home().ok_or_else(|| "can't tell the home folder: HOME isn't set".to_string());
    let paths = if args.paths.is_empty() { vec![home()?] } else { args.paths.clone() };
    let commands = paths.iter().map(|path| show(path)).collect::<Result<Vec<_>, _>>()?;
    let mut client = Client::connect(socket)?;
    for command in commands {
        client.request(command)?;
    }
    Ok(())
}

/// The window that shows `path`: the folder itself, or a file selected in
/// its folder.
fn show(path: &Path) -> Result<Command, String> {
    let path = absolute(path).map_err(|e| format!("can't open {}: {e}", path.display()))?;
    // The entry must exist; a link whose target is gone is still selected.
    path.symlink_metadata().map_err(|e| format!("can't open {}: {e}", path.display()))?;
    let (folder, select) = if path.is_dir() {
        (path, Vec::new())
    } else {
        let folder = path.parent().ok_or_else(|| format!("{} has no folder", path.display()))?.to_path_buf();
        (folder, vec![wire_text(&path)])
    };
    Ok(Command::ShowFolder { folder: wire_text(&folder), select })
}

/// `path` made absolute, with `.` and `..` worked out from the text alone.
fn absolute(path: &Path) -> std::io::Result<PathBuf> {
    let mut absolute = PathBuf::new();
    for component in std::path::absolute(path)?.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                absolute.pop();
            }
            other => absolute.push(other),
        }
    }
    Ok(absolute)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_opens_itself_and_a_file_its_folder() {
        let dir = nova_test_support::TempDir::new("cli-files");
        std::fs::create_dir_all(dir.join("Music")).unwrap();
        std::fs::write(dir.join("Music/song.flac"), b"").unwrap();
        std::os::unix::fs::symlink(dir.join("Music"), dir.join("Linked")).unwrap();
        let music = dir.join("Music").display().to_string();
        assert_eq!(
            show(&dir.join("Music/Albums/../.")).unwrap(),
            Command::ShowFolder { folder: music.clone(), select: vec![] }
        );
        std::os::unix::fs::symlink(dir.join("Gone"), dir.join("Music/dangling")).unwrap();
        assert_eq!(
            show(&dir.join("Music/dangling")).unwrap(),
            Command::ShowFolder { folder: music.clone(), select: vec![format!("{music}/dangling")] }
        );
        let linked = dir.join("Linked").display().to_string();
        assert_eq!(show(&dir.join("Linked")).unwrap(), Command::ShowFolder { folder: linked, select: vec![] });
        assert_eq!(
            show(&dir.join("Music/song.flac")).unwrap(),
            Command::ShowFolder { folder: music.clone(), select: vec![format!("{music}/song.flac")] }
        );
        assert!(show(&dir.join("missing")).unwrap_err().contains("can't open"));
    }

    /// A Latin-1 name, and a UTF-8 one using a character from the range
    /// raw bytes use, each go out as the wire text that names them.
    #[test]
    fn any_name_goes_out_as_wire_text() {
        use std::os::unix::ffi::OsStrExt;
        let dir = nova_test_support::TempDir::new("cli-files-wire");
        let pua = dir.join("\u{f7e9}");
        let latin1 = dir.join(std::ffi::OsStr::from_bytes(b"caf\xe9"));
        for folder in [pua, latin1] {
            std::fs::create_dir(&folder).unwrap();
            let Command::ShowFolder { folder: sent, .. } = show(&folder).unwrap() else { unreachable!() };
            assert_eq!(nova_protocol::wire_path(&sent), folder);
        }
    }
}
