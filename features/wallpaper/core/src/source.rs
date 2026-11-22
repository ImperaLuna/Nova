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

use nova_wallpaper_api::is_workshop_id;
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
/// Where a Steam install lists its other libraries.
const LIBRARY_FOLDERS: &str = "steamapps/libraryfolders.vdf";
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

/// The Steam libraries: Steam's installs under `home` that exist, and the
/// other libraries each lists in its `libraryfolders.vdf` (games on a
/// second disk).
pub fn steam_roots(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else { return Vec::new() };
    let mut roots: Vec<PathBuf> = Vec::new();
    for install in STEAM_ROOTS.iter().map(|root| home.join(root)).filter(|root| root.is_dir()) {
        let listed = std::fs::read_to_string(install.join(LIBRARY_FOLDERS)).unwrap_or_default();
        for root in std::iter::once(install).chain(library_paths(&listed)) {
            if root.is_dir() && !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    roots
}

/// The `"path"` values in a `libraryfolders.vdf` (Valve's KeyValues text:
/// one `"key"  "value"` pair per line).
fn library_paths(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|line| {
            let mut quoted = line.split('"').skip(1).step_by(2);
            (quoted.next()? == "path").then(|| quoted.next()).flatten()
        })
        // Backslashes are escaped there.
        .map(|path| PathBuf::from(path.replace("\\\\", "\\")))
        .filter(|path| path.is_absolute())
        .collect()
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
        let path = nova_settings::user_path(setting, text)?;
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
        // The extension first: an MP4's header also passes for a HEIF image.
        if is_video(&path) {
            return Ok(Kind::Video(path));
        }
        match image(&path)?.is_some() {
            true => Ok(Kind::Image(path)),
            false => Err(format!("{} isn't an image, a video or a Wallpaper Engine project", path.display())),
        }
    }
}

/// What a [`Source`] is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Image(PathBuf),
    /// A Wallpaper Engine project, with its preview when awww can show it.
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

fn is_video(path: &Path) -> bool {
    let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or_default();
    VIDEO_EXTENSIONS.iter().any(|video| video.eq_ignore_ascii_case(extension))
}

/// Which kind of image `path` is, if it's one awww can show: one whose
/// header gives its size (a file cut off before it isn't). Only the header
/// is read.
fn image(path: &Path) -> Result<Option<imagesize::ImageType>, String> {
    let mut header = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(HEADER_BYTES).read_to_end(&mut header))
        .map_err(|e| format!("can't read {}: {e}", path.display()))?;
    let kind = imagesize::image_type(&header).ok();
    Ok(kind.filter(|_| imagesize::size(path).is_ok()))
}

/// `project.json`'s fields Nova uses. Written by Wallpaper Engine and
/// Workshop authors: anything else in it is ignored.
#[derive(Deserialize)]
struct ProjectFile {
    #[serde(default)]
    preview: Option<String>,
}

/// The project in `folder`. A preview it names is used only when it's an
/// image inside the folder, and not a GIF: those are usually animated, and
/// awww would keep drawing one under the engine.
fn project(folder: PathBuf) -> Result<Kind, String> {
    let file = folder.join(PROJECT_FILE);
    let mut text = String::new();
    std::fs::File::open(&file)
        .and_then(|opened| opened.take(MAX_PROJECT_BYTES).read_to_string(&mut text))
        .map_err(|e| format!("can't read {}: {e}", file.display()))?;
    let parsed: ProjectFile =
        serde_json::from_str(&text).map_err(|e| format!("{} doesn't make sense: {e}", file.display()))?;
    let preview = parsed.preview.and_then(|name| preview(&folder, Path::new(&name)));
    Ok(Kind::Project { folder, preview })
}

/// The preview `name` in `folder`, if it's really inside it (also once
/// symlinks are followed) and an image awww can show that isn't a GIF.
fn preview(folder: &Path, name: &Path) -> Option<PathBuf> {
    if !inside(name) {
        return None;
    }
    let real = std::fs::canonicalize(folder.join(name)).ok()?;
    let shown = real.starts_with(std::fs::canonicalize(folder).ok()?)
        && real.is_file()
        && matches!(image(&real), Ok(Some(kind)) if kind != imagesize::ImageType::Gif);
    shown.then_some(real)
}

/// Whether a relative path stays inside the folder it's relative to.
fn inside(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|part| matches!(part, std::path::Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_test_support::TempDir;

    /// The smallest whole GIF: one transparent pixel.
    const GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";

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
        // An MP4's header, which also passes for a HEIF image.
        std::fs::write(dir.join("loop.MP4"), b"\x00\x00\x00\x20ftypisom\x00\x00\x02\x00isomiso2avc1mp41").unwrap();
        std::fs::write(dir.join("logo.gif"), GIF).unwrap();
        std::fs::write(dir.join("notes.toml"), "x = 1").unwrap();
        let kind = |name: &str| Source::from_request(&dir.join(name).display().to_string()).unwrap().kind(&[]);
        assert_eq!(kind("dusk.png"), Ok(Kind::Image(dir.join("dusk.png"))));
        assert_eq!(kind("loop.MP4"), Ok(Kind::Video(dir.join("loop.MP4"))));
        assert_eq!(kind("logo.gif"), Ok(Kind::Image(dir.join("logo.gif"))));
        assert!(kind("notes.toml").unwrap_err().contains("isn't an image, a video"));
        // A PNG cut off before its size.
        std::fs::write(dir.join("cut.png"), b"\x89PNG\r\n\x1a\n\x00\x00").unwrap();
        assert!(kind("cut.png").unwrap_err().contains("isn't an image, a video"));
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
        // Steam's other libraries are found through the install's list.
        let home = dir.join("home");
        let install = home.join(STEAM_ROOTS[0]);
        std::fs::create_dir_all(install.join("steamapps")).unwrap();
        let listed = format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
            install.display(),
            steam.display()
        );
        std::fs::write(install.join(LIBRARY_FOLDERS), listed).unwrap();
        assert_eq!(steam_roots(Some(&home)), [install, steam.clone()]);
        assert!(missing.contains("isn't downloaded"), "{missing}");
        // A preview outside the folder, or an animated one, isn't used.
        for preview in ["../preview.png", "/etc/passwd", "preview.gif"] {
            std::fs::write(item.join("preview.gif"), GIF).unwrap();
            std::fs::write(item.join(PROJECT_FILE), format!(r#"{{"preview": "{preview}"}}"#)).unwrap();
            assert_eq!(
                workshop.kind(std::slice::from_ref(&steam)),
                Ok(Kind::Project { folder: item.clone(), preview: None }),
                "{preview}"
            );
        }
        // Nor one that's a symlink out of it.
        std::os::unix::fs::symlink(dir.join("outside.png"), item.join("linked.png")).unwrap();
        write_png(&dir.join("outside.png"));
        std::fs::write(item.join(PROJECT_FILE), r#"{"preview": "linked.png"}"#).unwrap();
        assert_eq!(
            workshop.kind(std::slice::from_ref(&steam)),
            Ok(Kind::Project { folder: item.clone(), preview: None })
        );
        std::fs::write(item.join(PROJECT_FILE), "not json").unwrap();
        assert!(workshop.kind(&[steam]).unwrap_err().contains("doesn't make sense"));
    }
}
