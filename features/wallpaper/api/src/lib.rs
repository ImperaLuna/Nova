//! The wallpaper feature's messages: the wallpaper on every output or one
//! output (`nova wallpaper`), and the images in the wallpaper folder for
//! the carousel.
//!
//! A client sends a [`Command`] (`set_wallpaper`, `wallpapers`,
//! `wallpaper_folder`) and gets its reply; the feature publishes no events.
//! The work is `nova-wallpaper` (`features/wallpaper/core`).

use serde::{Deserialize, Serialize};

/// Sent by the UI and the CLI. Arguments with unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Shows `path` as the wallpaper on every output, or only on `output`
    /// (`nova wallpaper`): an absolute path to an image (feature
    /// `wallpaper`), or to a video or a Wallpaper Engine project folder, or
    /// a Steam Workshop ID (feature `wallpaper-engine`). Remembered across
    /// restarts until config.toml's `[wallpaper]` changes. Fails when it
    /// can't be shown.
    SetWallpaper {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    /// Replies with the [`Wallpapers`] shown now (feature `wallpaper` or
    /// `wallpaper-engine`).
    Wallpapers,
    /// Replies with the [`WallpaperFolder`]: the images in the wallpaper
    /// folder, for the carousel (feature `wallpaper`).
    WallpaperFolder,
}

/// Whether a `set_wallpaper` path is a Steam Workshop ID: only digits.
pub fn is_workshop_id(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// What `wallpapers` returns: the wallpaper every output shows, and the
/// outputs that show one of their own. Absolute paths or Workshop IDs;
/// empty when none is set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wallpapers {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub outputs: std::collections::BTreeMap<String, String>,
}

/// What `wallpaper_folder` returns: the wallpaper folder (config.toml's
/// `[wallpaper] folder`) and the images in it, sorted by name, at most
/// [`WALLPAPER_FOLDER_MAX_IMAGES`]. Absolute paths; no images when the folder
/// doesn't exist.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WallpaperFolder {
    pub folder: String,
    pub images: Vec<String>,
}

/// The most images `wallpaper_folder` lists: a carousel with more is no use,
/// and a reply's size is capped.
pub const WALLPAPER_FOLDER_MAX_IMAGES: usize = 1000;
