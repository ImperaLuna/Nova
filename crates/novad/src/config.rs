//! The user's `config.toml`.
//!
//! ```toml
//! # ~/.config/nova/config.toml
//! [theme]
//! name = "carbonfox"
//! mode = "dark"
//!
//! [ai-usage]
//! claude-usage-endpoint = true
//! codex-usage-endpoint = true
//!
//! [screenshot]
//! folder = "~/Pictures/Screenshots"
//!
//! [recording]
//! folder = "~/Videos/Recordings"
//! [quick-settings]
//! name = "Luna"
//!
//! [wallpaper]
//! path = "~/Pictures/Wallpapers/dusk.jpg"
//! [wallpaper.outputs]
//! DP-2 = "2861349227"  # a Wallpaper Engine Workshop item
//! ```

use crate::theme::ThemeChoice;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub theme: ThemeChoice,
    #[serde(default, rename = "ai-usage")]
    pub ai_usage: AiUsageConfig,
    #[serde(default)]
    pub screenshot: ScreenshotConfig,
    #[serde(default)]
    pub recording: RecordingConfig,
    #[serde(default, rename = "quick-settings")]
    pub quick_settings: QuickSettingsConfig,
    #[serde(default)]
    pub wallpaper: WallpaperConfig,
}

/// `[wallpaper]`: the wallpaper on every output, and ones for single
/// outputs by name. Absolute paths (or ones starting with `~/`) to an
/// image, or to a video or Wallpaper Engine project folder, or Workshop IDs
/// (those three need the `wallpaper-engine` feature). Nothing set means
/// Nova leaves the wallpaper alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WallpaperConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct QuickSettingsConfig {
    /// What the greeting calls the user, instead of the first name from
    /// their account; "" for no name.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct AiUsageConfig {
    /// Ask Anthropic for Claude's limits with Claude Code's sign-in. Off
    /// unless the user turns it on: Anthropic doesn't document that use.
    #[serde(default)]
    pub claude_usage_endpoint: bool,
    /// Ask OpenAI for Codex's limits with Codex's sign-in, so they stay
    /// current between Codex turns. Off unless the user turns it on: OpenAI
    /// doesn't document that use.
    #[serde(default)]
    pub codex_usage_endpoint: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ScreenshotConfig {
    /// Where screenshots are saved: an absolute path, or one starting with
    /// `~/`. By default `Screenshots` in the user's pictures folder.
    #[serde(default)]
    pub folder: Option<String>,
}

impl ScreenshotConfig {
    /// The folder screenshots go in (see [`folder`]).
    pub fn folder(&self) -> Result<PathBuf, String> {
        folder("screenshot", self.folder.as_deref(), || {
            nova_paths::user_dir("PICTURES", "Pictures").join(DEFAULT_SCREENSHOT_FOLDER)
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RecordingConfig {
    /// Where recordings are saved: an absolute path, or one starting with
    /// `~/`. By default `Recordings` in the user's videos folder.
    #[serde(default)]
    pub folder: Option<String>,
}

impl RecordingConfig {
    /// The folder recordings go in (see [`folder`]).
    pub fn folder(&self) -> Result<PathBuf, String> {
        folder("recording", self.folder.as_deref(), || {
            nova_paths::user_dir("VIDEOS", "Videos").join(DEFAULT_RECORDING_FOLDER)
        })
    }
}

/// The screenshot folder's name inside the pictures folder.
const DEFAULT_SCREENSHOT_FOLDER: &str = "Screenshots";
/// The recording folder's name inside the videos folder.
const DEFAULT_RECORDING_FOLDER: &str = "Recordings";

/// `[section] folder`, or `default` when it isn't set. One that isn't
/// absolute (or under `~/`) is an error, so a typo doesn't scatter files in
/// novad's folder.
fn folder(section: &str, configured: Option<&str>, default: impl FnOnce() -> PathBuf) -> Result<PathBuf, String> {
    match configured {
        Some(folder) => user_path(&format!("[{section}] folder"), folder),
        None => Ok(default()),
    }
}

/// `path` from config.toml as an absolute path, `~/` meaning the home
/// folder. `setting` names it in the error for one that isn't absolute.
pub fn user_path(setting: &str, path: &str) -> Result<PathBuf, String> {
    let resolved = match path.strip_prefix("~/") {
        Some(rest) => nova_paths::home().ok_or("HOME isn't set, so ~ means nothing")?.join(rest),
        None => PathBuf::from(path),
    };
    match resolved.is_absolute() {
        true => Ok(resolved),
        false => Err(format!("{setting} {path:?} isn't an absolute path or one under ~/")),
    }
}

/// The settings in `path`; a missing file means all defaults.
pub fn read(path: &Path) -> Result<Config, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(format!("can't read {}: {e}", path.display())),
    };
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoints_are_off_unless_turned_on() {
        let config: Config = toml::from_str("[theme]\nname = \"carbonfox\"").unwrap();
        assert!(!config.ai_usage.claude_usage_endpoint && !config.ai_usage.codex_usage_endpoint);
        let config: Config = toml::from_str("[ai-usage]\nclaude-usage-endpoint = true").unwrap();
        assert!(config.ai_usage.claude_usage_endpoint && !config.ai_usage.codex_usage_endpoint);
        let config: Config = toml::from_str("[ai-usage]\ncodex-usage-endpoint = true").unwrap();
        assert!(config.ai_usage.codex_usage_endpoint);
    }

    #[test]
    fn the_screenshot_folder_must_be_absolute() {
        let folder = |text: &str| toml::from_str::<Config>(text).unwrap().screenshot.folder();
        assert_eq!(folder("[screenshot]\nfolder = \"/data/shots\""), Ok(PathBuf::from("/data/shots")));
        assert!(folder("[screenshot]\nfolder = \"~/Shots\"").unwrap().ends_with("Shots"));
        assert!(folder("[screenshot]\nfolder = \"Shots\"").unwrap_err().contains("absolute"));
        assert!(folder("").unwrap().ends_with("Screenshots"));
        let recordings = |text: &str| toml::from_str::<Config>(text).unwrap().recording.folder();
        assert!(recordings("").unwrap().ends_with("Recordings"));
        assert!(recordings("[recording]\nfolder = \"Videos\"").unwrap_err().contains("[recording]"));
    }

    #[test]
    fn wallpapers_are_unset_unless_configured() {
        assert_eq!(toml::from_str::<Config>("").unwrap().wallpaper, WallpaperConfig::default());
        let text = "[wallpaper]\npath = \"~/a.jpg\"\n[wallpaper.outputs]\nDP-2 = \"/b.png\"";
        let wallpaper = toml::from_str::<Config>(text).unwrap().wallpaper;
        assert_eq!(wallpaper.path.as_deref(), Some("~/a.jpg"));
        assert_eq!(wallpaper.outputs["DP-2"], "/b.png");
        assert!(toml::from_str::<Config>("[wallpaper]\nimage = \"/a.jpg\"").is_err());
    }

    #[test]
    fn the_greeting_name_is_the_accounts_unless_set() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.quick_settings.name, None);
        let config: Config = toml::from_str("[quick-settings]\nname = \"Katov\"").unwrap();
        assert_eq!(config.quick_settings.name.as_deref(), Some("Katov"));
    }
}
