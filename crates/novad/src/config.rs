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
//! ```

use crate::theme::ThemeChoice;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub theme: ThemeChoice,
    #[serde(default, rename = "ai-usage")]
    pub ai_usage: AiUsageConfig,
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
}
