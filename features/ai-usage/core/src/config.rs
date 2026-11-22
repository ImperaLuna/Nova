//! config.toml's `[ai-usage]`: whether to ask the usage endpoints. novad
//! reads the file; this is the section's shape.

use serde::Deserialize;

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
