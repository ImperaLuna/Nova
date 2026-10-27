//! Running linux-wallpaperengine (feature `wallpaper-engine`): one process
//! shows every output's dynamic wallpaper, each with `--screen-root <output>
//! --bg <project>`, and is started again whenever that set changes.
//!
//! Flags from the IMP-168 spike on niri: the background layer (its default,
//! bottom, is cloned onto every workspace in niri's overview), pausing only
//! under an active fullscreen window (otherwise any output-sized window,
//! even on a hidden workspace, freezes every screen), no sound, and an
//! assets folder: Wallpaper Engine's own from Steam when it's installed,
//! otherwise an empty one, which simple scenes and videos don't need more
//! than.
//!
//! It only opens projects, and only finds files that are really inside the
//! project's folder (not through a symlink), so a video gets a project of
//! its own in the state folder with a hard link to it, or a copy when it's
//! on another disk or a link isn't allowed.

use super::source::PROJECT_FILE;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PROGRAM: &str = "linux-wallpaperengine";
const FLAGS: &[&str] = &[
    "--silent",
    "--layer",
    "background",
    "--fullscreen-pause-only-active",
    // Fills the output and crops, like awww's images.
    "--scaling",
    "fill",
];
/// Wallpaper Engine's assets in a Steam library.
const STEAM_ASSETS: &str = "steamapps/common/wallpaper_engine/assets";
/// Nova's folder in the state folder: the empty assets folder and the
/// videos' projects.
const STATE_FOLDER: &str = "wallpaper-engine";
const EMPTY_ASSETS: &str = "assets";
const VIDEO_PROJECTS: &str = "videos";
/// NixOS keeps the GPU driver's libraries here, outside every package, and
/// nixpkgs' linux-wallpaperengine doesn't look there: without it, NVIDIA
/// video decoding falls back to a copy on the CPU (15% of a core instead of
/// 3.7% in the IMP-168 spike). Remove once nixpkgs wraps the program with
/// the driver path.
const NIXOS_DRIVER_LIBS: &str = "/run/opengl-driver/lib";

/// Where its files go: `<state>/wallpaper-engine`.
pub fn state_folder(state_dir: &Path) -> PathBuf {
    state_dir.join(STATE_FOLDER)
}

/// The command that shows `screens` (output name to project folder).
pub fn command(screens: &BTreeMap<String, PathBuf>, assets: &Path, path_var: &str) -> Command {
    let mut command = Command::new(PROGRAM);
    command.env("PATH", path_var).args(FLAGS).arg("--assets-dir").arg(assets);
    for (output, project) in screens {
        command.arg("--screen-root").arg(output).arg("--bg").arg(project);
    }
    if Path::new(NIXOS_DRIVER_LIBS).is_dir() {
        let mut libraries = OsString::from(NIXOS_DRIVER_LIBS);
        if let Some(more) = std::env::var_os("LD_LIBRARY_PATH").filter(|more| !more.is_empty()) {
            libraries.push(":");
            libraries.push(more);
        }
        command.env("LD_LIBRARY_PATH", libraries);
    }
    command
}

/// Wallpaper Engine's assets from the first Steam library that has them,
/// else an empty folder in `folder` (made if missing).
pub fn assets(steam_roots: &[PathBuf], folder: &Path) -> Result<PathBuf, String> {
    if let Some(steam) = steam_roots.iter().map(|root| root.join(STEAM_ASSETS)).find(|assets| assets.is_dir()) {
        return Ok(steam);
    }
    let empty = folder.join(EMPTY_ASSETS);
    std::fs::create_dir_all(&empty).map_err(|e| format!("can't make {}: {e}", empty.display()))?;
    Ok(empty)
}

/// The project that plays `video`, made in `folder` if it isn't there yet.
/// Named after the video's name, size and time, so a changed video gets a
/// new one.
pub fn video_project(video: &Path, folder: &Path) -> Result<PathBuf, String> {
    let metadata = std::fs::metadata(video).map_err(|e| format!("can't read {}: {e}", video.display()))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs());
    let stem = video.file_stem().unwrap_or_default().to_string_lossy();
    let extension = video.extension().unwrap_or_default().to_string_lossy();
    let videos = folder.join(VIDEO_PROJECTS);
    let project = videos.join(format!("{stem}-{}-{modified}", metadata.len()));
    if project.join(PROJECT_FILE).is_file() {
        return Ok(project);
    }
    // Made under another name and renamed into place, so a half-made one
    // is never used.
    let making = videos.join(format!(".making-{}", std::process::id()));
    let made = (|| {
        let _ = std::fs::remove_dir_all(&making);
        std::fs::create_dir_all(&making)?;
        let file = format!("video.{extension}");
        if std::fs::hard_link(video, making.join(&file)).is_err() {
            std::fs::copy(video, making.join(&file))?;
        }
        let description = serde_json::json!({ "title": stem, "type": "video", "file": file });
        std::fs::write(making.join(PROJECT_FILE), description.to_string())?;
        std::fs::rename(&making, &project)
    })();
    made.map_err(|e| {
        let _ = std::fs::remove_dir_all(&making);
        format!("can't make a project for {} in {}: {e}", video.display(), videos.display())
    })?;
    Ok(project)
}

/// Removes the videos' projects in `folder` that aren't in `keep`.
pub fn forget_videos(folder: &Path, keep: &BTreeSet<PathBuf>) -> Result<(), String> {
    let videos = folder.join(VIDEO_PROJECTS);
    let entries = match std::fs::read_dir(&videos) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("can't read {}: {e}", videos.display())),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !keep.contains(&path) {
            std::fs::remove_dir_all(&path).map_err(|e| format!("can't remove {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    #[test]
    fn the_command_shows_each_output_its_project_on_the_background_layer() {
        let screens = [("DP-1".to_owned(), PathBuf::from("/w/tide")), ("DP-2".to_owned(), PathBuf::from("/w/rain"))];
        let command = command(&screens.into(), Path::new("/assets"), "/bin");
        let args: Vec<_> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        let line = args.join(" ");
        assert!(line.contains("--layer background") && line.contains("--fullscreen-pause-only-active"), "{line}");
        assert!(line.contains("--silent") && line.contains("--assets-dir /assets"), "{line}");
        assert!(line.ends_with("--screen-root DP-1 --bg /w/tide --screen-root DP-2 --bg /w/rain"), "{line}");
        // --scaling applies to the screens after it.
        assert!(line.find("--scaling").unwrap() < line.find("--screen-root").unwrap());
    }

    #[test]
    fn steam_assets_are_used_when_installed() {
        let dir = TempDir::new("engine-assets");
        let steam = dir.join("steam");
        assert_eq!(assets(std::slice::from_ref(&steam), &dir.join("nova")), Ok(dir.join("nova").join(EMPTY_ASSETS)));
        assert!(dir.join("nova").join(EMPTY_ASSETS).is_dir());
        std::fs::create_dir_all(steam.join(STEAM_ASSETS)).unwrap();
        assert_eq!(assets(std::slice::from_ref(&steam), &dir.join("nova")), Ok(steam.join(STEAM_ASSETS)));
    }

    #[test]
    fn a_video_gets_a_project_of_its_own_until_it_isnt_used() {
        let dir = TempDir::new("engine-videos");
        let video = dir.join("rain.webm");
        std::fs::write(&video, "frames").unwrap();
        let folder = dir.join("nova");
        let project = video_project(&video, &folder).unwrap();
        let description: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(project.join(PROJECT_FILE)).unwrap()).unwrap();
        assert_eq!(description["type"], "video");
        assert_eq!(std::fs::read_to_string(project.join(description["file"].as_str().unwrap())).unwrap(), "frames");
        assert!(!project.join("video.webm").is_symlink(), "linux-wallpaperengine doesn't follow symlinks");
        assert_eq!(video_project(&video, &folder).unwrap(), project, "made once");
        // A changed video gets a new one; the old one goes once unused.
        std::fs::write(&video, "more frames").unwrap();
        let changed = video_project(&video, &folder).unwrap();
        assert_ne!(changed, project);
        forget_videos(&folder, &[changed.clone()].into()).unwrap();
        assert!(!project.exists() && changed.is_dir());
        assert!(video_project(&dir.join("missing.mp4"), &folder).unwrap_err().contains("can't read"));
    }
}
