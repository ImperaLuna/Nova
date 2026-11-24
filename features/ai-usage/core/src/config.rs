use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct AiUsageConfig {
    #[serde(default)]
    pub claude_usage_endpoint: bool,
    #[serde(default)]
    pub codex_usage_endpoint: bool,
}
