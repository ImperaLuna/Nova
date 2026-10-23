//! AI usage (feature `ai-usage`): how much of their Claude plan limits the
//! user has used. Only official, local sources: Claude Code hands its limits
//! to its status line command (`nova claude-statusline`). Nothing here makes
//! network requests or reads the tool's credentials.

mod claude;

pub use claude::limits_from_statusline;

/// The feature manifest's id.
pub const FEATURE_ID: &str = "ai-usage";

fn round_percent(used: f64) -> u8 {
    used.round().clamp(0.0, 100.0) as u8
}
