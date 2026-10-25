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
//! ```

use crate::theme::ThemeChoice;
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
    /// The folder screenshots go in. One that isn't absolute (or under
    /// `~/`) is an error, so a typo doesn't scatter files in novad's folder.
    pub fn folder(&self) -> Result<PathBuf, String> {
        let Some(folder) = &self.folder else {
            return Ok(nova_paths::user_dir("PICTURES", "Pictures").join(DEFAULT_SCREENSHOT_FOLDER));
        };
        let path = match folder.strip_prefix("~/") {
            Some(rest) => {
                PathBuf::from(std::env::var_os("HOME").ok_or("HOME isn't set, so ~ means nothing")?).join(rest)
            }
            None => PathBuf::from(folder),
        };
        match path.is_absolute() {
            true => Ok(path),
            false => Err(format!("[screenshot] folder {folder:?} isn't an absolute path or one under ~/")),
        }
    }
}

/// The screenshot folder's name inside the pictures folder.
const DEFAULT_SCREENSHOT_FOLDER: &str = "Screenshots";

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
    }
}
