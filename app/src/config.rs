use nova_ai_usage::AiUsageConfig;
use nova_recording::RecordingConfig;
use nova_screenshot::ScreenshotConfig;
use nova_theme::ThemeChoice;
use nova_wallpaper::WallpaperConfig;
use serde::Deserialize;
use std::path::Path;

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
    #[serde(default, rename = "files")]
    _files: Option<serde::de::IgnoredAny>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct QuickSettingsConfig {
    #[serde(default)]
    pub name: Option<String>,
}

pub fn read(path: &Path) -> Result<Config, String> {
    nova_settings::read_toml(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
    fn the_example_config_sets_every_section() {
        let config: Config = toml::from_str(include_str!("../../packaging/config.example.toml")).unwrap();
        assert_eq!(config.quick_settings.name.as_deref(), Some("Luna"));
        assert_ne!(config.wallpaper, WallpaperConfig::default());
        assert!(config.screenshot.folder().is_ok() && config.recording.folder().is_ok());
    }

    #[test]
    fn the_greeting_name_is_the_login_unless_set() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.quick_settings.name, None);
        let config: Config = toml::from_str("[quick-settings]\nname = \"Katov\"").unwrap();
        assert_eq!(config.quick_settings.name.as_deref(), Some("Katov"));
    }
}
