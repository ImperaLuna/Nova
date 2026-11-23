//! The AI usage feature's own values: the ones nothing else draws with.
//! Everything shared is `nova_look::tokens`.
//!
//! Until the Quickshell shell is removed (IMP-368) the same values live in
//! `ui/theme/Tokens.qml`; a test holds the two equal.

use std::time::Duration;

/// The `%` after the panel's headline number, as a share of its size.
pub const HERO_UNIT_SCALE: f32 = 0.42;

/// How often the "left" and "resets" texts count down while the panel is
/// open (`countdownInterval` in `ui/ai_usage/AiUsage.qml`).
pub const COUNTDOWN: Duration = Duration::from_secs(30);

#[cfg(test)]
mod tests {
    use super::*;

    const TOKENS_QML: &str = include_str!("../../../../ui/theme/Tokens.qml");
    const AI_USAGE_QML: &str = include_str!("../../../../ui/ai_usage/AiUsage.qml");

    #[test]
    fn the_values_are_today_s_shell_s() {
        assert!(TOKENS_QML.contains(&format!("readonly property real heroUnitScale: {HERO_UNIT_SCALE}")));
        let seconds = COUNTDOWN.as_secs();
        assert!(AI_USAGE_QML.contains(&format!("countdownInterval: {seconds} * 1000")));
    }
}
