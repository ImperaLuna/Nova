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
//!
//! [quick-settings]
//! name = "Luna"
//!
//! [wallpaper]
//! path = "~/Pictures/Wallpapers/dusk.jpg"
//! folder = "~/Pictures/Wallpapers"
//! [wallpaper.outputs]
//! DP-2 = "2861349227"  # a Wallpaper Engine Workshop item
//! ```

use crate::theme::ThemeChoice;
// `[wallpaper]` is the wallpaper feature's (`nova-wallpaper`).
pub use nova_screenshot::ScreenshotConfig;
pub use nova_wallpaper::WallpaperConfig;
use serde::Deserialize;
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
    /// `[files]` set the look of the file manager Nova used to have; Nova
    /// Files is its own app now. Ignored, so a config that still has it
    /// loads.
    #[serde(default, rename = "files")]
    _files: Option<serde::de::IgnoredAny>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct QuickSettingsConfig {
    /// What the greeting calls the user, instead of their login name; ""
    /// for no name.
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

/// The recording folder's name inside the videos folder.
const DEFAULT_RECORDING_FOLDER: &str = "Recordings";

// Reading the file and its folders is the framework's (IMP-372).
use nova_settings::folder;

/// The settings in `path`; a missing file means all defaults.
pub fn read(path: &Path) -> Result<Config, String> {
    nova_settings::read_toml(path)
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
        let wallpapers = |text: &str| toml::from_str::<Config>(text).unwrap().wallpaper.folder();
        assert!(wallpapers("").unwrap().ends_with("Wallpapers"));
        assert_eq!(wallpapers("[wallpaper]\nfolder = \"/art\""), Ok(PathBuf::from("/art")));
        assert!(wallpapers("[wallpaper]\nfolder = \"art\"").unwrap_err().contains("[wallpaper]"));
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
    fn an_old_files_section_still_loads() {
        let config = toml::from_str::<Config>("[files]\nframe = 40\nlist = 100\n[screenshot]\nfolder = \"~/Shots\"");
        assert_eq!(config.unwrap().screenshot.folder.as_deref(), Some("~/Shots"));
    }

    #[test]
    fn the_greeting_name_is_the_login_unless_set() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.quick_settings.name, None);
        let config: Config = toml::from_str("[quick-settings]\nname = \"Katov\"").unwrap();
        assert_eq!(config.quick_settings.name.as_deref(), Some("Katov"));
    }
}
