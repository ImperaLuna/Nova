use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

const DEFAULT_WALLPAPER_FOLDER: &str = "Wallpapers";

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
    pub fn folder(&self) -> Result<PathBuf, String> {
        nova_settings::folder("wallpaper", self.folder.as_deref(), || {
            nova_paths::user_dir("PICTURES", "Pictures").join(DEFAULT_WALLPAPER_FOLDER)
        })
    }

    pub fn images(&self) -> Self {
        Self { folder: None, ..self.clone() }
    }
}
