//! What a wallpaper setting names, and what it turns out to be.
//!
//! A setting is an absolute path or a Steam Workshop ID (only digits). An
//! image goes to awww. A Wallpaper Engine project (a folder with
//! `project.json`), a downloaded Workshop item (a project in Steam's
//! Workshop folder) or a video is dynamic: it goes to linux-wallpaperengine
//! (`engine`), and a project's still preview, when it has one, goes to awww
//! underneath, so it fades in first and shows while the engine starts.
//! What a setting is gets looked up afresh each time, since the files can
//! change.

use crate::config;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Where a Steam library may be, under the home folder: Steam's own
/// install, its old link, Flatpak and Snap (the places linux-wallpaperengine
/// looks too).
const STEAM_ROOTS: &[&str] = &[
    ".local/share/Steam",
    ".steam/steam",
    ".var/app/com.valvesoftware.Steam/.local/share/Steam",
    "snap/steam/common/.local/share/Steam",
];
/// Wallpaper Engine's Workshop downloads in a Steam library (app 431960).
const WORKSHOP: &str = "steamapps/workshop/content/431960";
/// The file that makes a folder a Wallpaper Engine project.
pub const PROJECT_FILE: &str = "project.json";
/// More than any real `project.json`; a bigger one isn't read.
const MAX_PROJECT_BYTES: u64 = 1 << 20;
/// Enough of a file to tell which kind of image it is.
const HEADER_BYTES: u64 = 64;
/// Files shown as videos, by extension.
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "webm", "mkv", "mov", "m4v", "avi"];

/// The Steam libraries under `home` that exist.
pub fn steam_roots(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else { return Vec::new() };
    STEAM_ROOTS.iter().map(|root| home.join(root)).filter(|root| root.is_dir()).collect()
}

/// A wallpaper setting: an absolute path, or a Workshop ID.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Source(String);

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Source {
    /// A setting from config.toml: a Workshop ID, or a path that's absolute
    /// or starts with `~/`.
    pub fn from_config(setting: &str, text: &str) -> Result<Self, String> {
        if is_workshop_id(text) {
            return Ok(Self(text.to_owned()));
        }
        let path = config::user_path(setting, text)?;
        path.to_str().map(|path| Self(path.to_owned())).ok_or_else(|| format!("{setting} {text:?} isn't valid UTF-8"))
    }

    /// A setting from `nova wallpaper`: a Workshop ID or an absolute path.
    pub fn from_request(text: &str) -> Result<Self, String> {
        match is_workshop_id(text) || Path::new(text).is_absolute() {
            true => Ok(Self(text.to_owned())),
            false => Err(format!("{text} isn't an absolute path or a Workshop ID")),
        }
    }

    /// What it is now, looking Workshop IDs up in `steam_roots`; an error
    /// says why it can't be shown.
    pub fn kind(&self, steam_roots: &[PathBuf]) -> Result<Kind, String> {
        if is_workshop_id(&self.0) {
            let found = steam_roots
                .iter()
                .map(|root| root.join(WORKSHOP).join(&self.0))
                .find(|folder| folder.join(PROJECT_FILE).is_file());
            let folder = found.ok_or_else(|| {
                format!("Workshop item {} isn't downloaded (subscribe to it in Wallpaper Engine on Steam)", self.0)
            })?;
            return project(folder);
        }
        let path = PathBuf::from(&self.0);
        // Looked at before it's opened: opening a FIFO waits for a writer.
        let metadata = std::fs::metadata(&path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
        if metadata.is_dir() {
            return match path.join(PROJECT_FILE).is_file() {
                true => project(path),
                false => Err(format!("{} isn't a Wallpaper Engine project (it has no {PROJECT_FILE})", path.display())),
            };
        }
        if !metadata.is_file() {
            return Err(format!("{} isn't a file", path.display()));
        }
        match still_image(&path)? {
            true => Ok(Kind::Image(path)),
            false if is_video(&path) => Ok(Kind::Video(path)),
            false => Err(format!("{} isn't an image, a video or a Wallpaper Engine project", path.display())),
        }
    }
}

/// What a [`Source`] is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Image(PathBuf),
    /// A Wallpaper Engine project, with its preview when it's a still image.
    Project {
        folder: PathBuf,
        preview: Option<PathBuf>,
    },
    Video(PathBuf),
}

impl Kind {
    /// Shown by linux-wallpaperengine.
    pub fn is_dynamic(&self) -> bool {
        !matches!(self, Self::Image(_))
    }
}

fn is_workshop_id(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_video(path: &Path) -> bool {
    let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or_default();
    VIDEO_EXTENSIONS.iter().any(|video| video.eq_ignore_ascii_case(extension))
}

/// Whether `path` is a still image awww can show (an animated GIF isn't:
/// awww would keep drawing it). Only the header is read.
fn still_image(path: &Path) -> Result<bool, String> {
    let mut header = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(HEADER_BYTES).read_to_end(&mut header))
        .map_err(|e| format!("can't read {}: {e}", path.display()))?;
    Ok(imagesize::image_type(&header).is_ok_and(|kind| kind != imagesize::ImageType::Gif))
}

/// `project.json`'s fields Nova uses. Written by Wallpaper Engine and
/// Workshop authors: anything else in it is ignored.
#[derive(Deserialize)]
struct ProjectFile {
    #[serde(default)]
    preview: Option<String>,
}

/// The project in `folder`. A preview it names is used only when it's a
/// still image inside the folder.
fn project(folder: PathBuf) -> Result<Kind, String> {
    let file = folder.join(PROJECT_FILE);
    let mut text = String::new();
    std::fs::File::open(&file)
        .and_then(|opened| opened.take(MAX_PROJECT_BYTES).read_to_string(&mut text))
        .map_err(|e| format!("can't read {}: {e}", file.display()))?;
    let parsed: ProjectFile =
        serde_json::from_str(&text).map_err(|e| format!("{} doesn't make sense: {e}", file.display()))?;
    let preview = parsed
        .preview
        .filter(|name| inside(Path::new(name)))
        .map(|name| folder.join(name))
        .filter(|preview| preview.is_file() && still_image(preview).unwrap_or(false));
    Ok(Kind::Project { folder, preview })
}

/// Whether a relative path stays inside the folder it's relative to.
fn inside(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|part| matches!(part, std::path::Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    fn write_png(path: &Path) {
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(file, 1, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.write_header().unwrap().write_image_data(&[0, 0, 0]).unwrap();
    }

    #[test]
    fn settings_are_paths_or_workshop_ids() {
        assert_eq!(Source::from_request("2861349227").unwrap().to_string(), "2861349227");
        assert!(Source::from_request("/w/dusk.jpg").is_ok());
        assert!(Source::from_request("dusk.jpg").unwrap_err().contains("absolute path or a Workshop ID"));
        assert!(Source::from_request("").is_err());
        assert!(Source::from_config("[wallpaper] path", "12a").unwrap_err().contains("absolute"));
    }

    #[test]
    fn each_kind_is_told_apart() {
        let dir = TempDir::new("wallpaper-kinds");
        write_png(&dir.join("dusk.png"));
        std::fs::write(dir.join("loop.MP4"), "not really a video").unwrap();
        std::fs::write(dir.join("loop.gif"), b"GIF89a\x01\x00\x01\x00").unwrap();
        std::fs::write(dir.join("notes.toml"), "x = 1").unwrap();
        let kind = |name: &str| Source::from_request(&dir.join(name).display().to_string()).unwrap().kind(&[]);
        assert_eq!(kind("dusk.png"), Ok(Kind::Image(dir.join("dusk.png"))));
        assert_eq!(kind("loop.MP4"), Ok(Kind::Video(dir.join("loop.MP4"))));
        assert!(kind("loop.gif").unwrap_err().contains("isn't an image, a video"), "an animated GIF isn't still");
        assert!(kind("notes.toml").unwrap_err().contains("isn't an image, a video"));
        assert!(kind("missing.png").unwrap_err().contains("can't read"));
        assert!(kind("").unwrap_err().contains("no project.json"), "a plain folder");
    }

    #[test]
    fn projects_come_with_a_still_preview_inside_them() {
        let dir = TempDir::new("wallpaper-projects");
        let steam = dir.join("steam");
        let item = steam.join(WORKSHOP).join("2861349227");
        std::fs::create_dir_all(&item).unwrap();
        write_png(&item.join("preview.png"));
        std::fs::write(item.join(PROJECT_FILE), r#"{"title": "Tide", "type": "scene", "preview": "preview.png"}"#)
            .unwrap();
        let workshop = Source::from_request("2861349227").unwrap();
        assert_eq!(
            workshop.kind(std::slice::from_ref(&steam)),
            Ok(Kind::Project { folder: item.clone(), preview: Some(item.join("preview.png")) })
        );
        let missing = Source::from_request("1").unwrap().kind(std::slice::from_ref(&steam)).unwrap_err();
        assert!(missing.contains("isn't downloaded"), "{missing}");
        // A preview outside the folder, or an animated one, isn't used.
        for preview in ["../preview.png", "/etc/passwd", "preview.gif"] {
            std::fs::write(item.join("preview.gif"), b"GIF89a\x01\x00\x01\x00").unwrap();
            std::fs::write(item.join(PROJECT_FILE), format!(r#"{{"preview": "{preview}"}}"#)).unwrap();
            assert_eq!(
                workshop.kind(std::slice::from_ref(&steam)),
                Ok(Kind::Project { folder: item.clone(), preview: None }),
                "{preview}"
            );
        }
        std::fs::write(item.join(PROJECT_FILE), "not json").unwrap();
        assert!(workshop.kind(&[steam]).unwrap_err().contains("doesn't make sense"));
    }
}
