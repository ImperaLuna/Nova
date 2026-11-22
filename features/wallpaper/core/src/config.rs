//! config.toml's `[wallpaper]`: the wallpapers it sets, and the folder the
//! carousel lists. novad reads the file; this is the section's shape.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The wallpaper folder's name inside the pictures folder.
const DEFAULT_WALLPAPER_FOLDER: &str = "Wallpapers";

/// `[wallpaper]`: the wallpaper on every output, and ones for single
/// outputs by name. Absolute paths (or ones starting with `~/`) to an
/// image, or to a video or Wallpaper Engine project folder, or Workshop IDs
/// (those three need the `wallpaper-engine` feature). Nothing set means
/// Nova leaves the wallpaper alone. `folder` is where the carousel finds
/// images to pick from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WallpaperConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

impl WallpaperConfig {
    /// The folder the carousel lists (see [`nova_settings::folder`]): by default
    /// `Wallpapers` in the user's pictures folder.
    pub fn folder(&self) -> Result<PathBuf, String> {
        nova_settings::folder("wallpaper", self.folder.as_deref(), || {
            nova_paths::user_dir("PICTURES", "Pictures").join(DEFAULT_WALLPAPER_FOLDER)
        })
    }

    /// The images it sets, without the folder, which only lists images to
    /// pick from: changing that doesn't undo a pick.
    pub fn images(&self) -> Self {
        Self { folder: None, ..self.clone() }
    }
}
