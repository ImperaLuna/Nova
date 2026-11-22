//! The AI usage feature's messages: how much of their Claude and Codex plan
//! limits the user has used, for the bar's pill and its panel.
//!
//! A client sends a [`Command`] (`show_ai_usage`, `refresh_ai_usage`,
//! `report_claude_usage`); novad publishes an [`Event`] on [`TOPIC`] right
//! after subscribing and whenever a number changes. The work is
//! `nova-ai-usage` (`features/ai-usage/core`).

use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};

/// The topic a client subscribes to for [`Event::AiUsage`].
pub const TOPIC: Topic = Topic::named("ai_usage");

/// Sent by the UI and `nova claude-statusline`. Arguments with unknown
/// fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Picks the provider the bar shows (feature `ai-usage`). Remembered
    /// across restarts.
    ShowAiUsage { provider: AiProvider },
    /// Asks for fresh numbers now, when the panel opens (feature
    /// `ai-usage`). Asking again within a minute does nothing.
    RefreshAiUsage,
    /// Claude's limits, read from Claude Code's status line by
    /// `nova claude-statusline` (feature `ai-usage`).
    ReportClaudeUsage { limits: Vec<UsageLimit> },
}

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// Plan usage of AI coding tools, sent right after subscribing and
    /// whenever a number changes.
    AiUsage(AiUsage),
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiUsage {
    /// The provider the bar shows.
    pub shown: AiProvider,
    /// Every provider, in display order.
    pub providers: Vec<ProviderUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    Claude,
    Codex,
}

impl AiProvider {
    pub const ALL: [AiProvider; 2] = [AiProvider::Claude, AiProvider::Codex];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub provider: AiProvider,
    pub state: UsageState,
    /// The subscription's display name, like `Plus`, when the source says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// The windows the plan has right now, session first. Plans differ:
    /// some have only a weekly limit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limits: Vec<UsageLimit>,
    /// Why there are no numbers and how to get them, when unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageState {
    /// novad is still reading the source.
    Loading,
    Ready,
    /// No numbers; `hint` says why.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageLimit {
    pub window: UsageWindow,
    /// 0 to 100.
    pub percent: u8,
    /// When the window starts over, RFC 3339 UTC. Absent once it has, until
    /// the tool is used again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageWindow {
    /// The rolling 5-hour limit.
    Session,
    Weekly,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_event_is_on_its_topic() {
        let usage = AiUsage { shown: AiProvider::Claude, providers: vec![] };
        assert_eq!(Event::AiUsage(usage).topic(), Some(TOPIC));
    }
}
