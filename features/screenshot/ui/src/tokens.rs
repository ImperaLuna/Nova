//! The screenshot picker's and card's own sizes and timings. Until the
//! Quickshell shell is removed the same values live in
//! `ui/theme/Tokens.qml`, under the names the test below lists: change a
//! value in both.

use gpui::{Pixels, px};
use std::time::Duration;

/// The dim over the frozen screens outside the region (black at this
/// opacity), and the region's outline.
pub const VEIL_OPACITY: f32 = 0.45;
pub const OUTLINE: Pixels = px(1.5);
/// Smaller than this either way is a click, not a region.
pub const MIN_DRAG: Pixels = px(4.0);
/// The toolbar at the top, where the central pill is, and its buttons.
pub const BAR_HEIGHT: Pixels = px(44.0);
pub const BUTTON_HEIGHT: Pixels = px(32.0);
pub const BUTTON_PAD: Pixels = px(12.0);
/// The region's size, under its corner.
pub const BADGE_HEIGHT: Pixels = px(26.0);
/// The card a saved shot lands in, at the screen's bottom right.
pub const CARD_WIDTH: Pixels = px(360.0);
pub const THUMB_HEIGHT: Pixels = px(184.0);
pub const CARD_BUTTON_HEIGHT: Pixels = px(36.0);
pub const CARD_GAP: Pixels = px(12.0);
/// How long the card stays; the pointer on it holds it.
pub const CARD_TIME: Duration = Duration::from_millis(6000);
/// The line along its bottom that drains over that time, moving every
/// COUNTDOWN_TICK.
pub const DRAIN_HEIGHT: Pixels = px(3.0);
pub const DRAIN_OPACITY: f32 = 0.55;
pub const COUNTDOWN_TICK: Duration = Duration::from_millis(83);

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const TOKENS_QML: &str = include_str!("../../../../ui/theme/Tokens.qml");

    fn qml_properties() -> HashMap<&'static str, &'static str> {
        TOKENS_QML
            .lines()
            .filter_map(|line| {
                let mut words = line.trim().strip_prefix("readonly property ")?.splitn(2, ' ');
                let _kind = words.next()?;
                let (name, value) = words.next()?.split_once(':')?;
                Some((name.trim(), value.trim()))
            })
            .collect()
    }

    #[test]
    fn every_token_has_tokens_qml_s_value() {
        let pixels = |value: Pixels| f64::from(f32::from(value));
        let ms = |value: Duration| value.as_millis() as f64;
        let ours: &[(&str, f64)] = &[
            ("captureVeilOpacity", f64::from(VEIL_OPACITY)),
            ("captureOutline", pixels(OUTLINE)),
            ("captureMinDrag", pixels(MIN_DRAG)),
            ("captureBarHeight", pixels(BAR_HEIGHT)),
            ("captureButtonHeight", pixels(BUTTON_HEIGHT)),
            ("captureButtonPad", pixels(BUTTON_PAD)),
            ("captureBadgeHeight", pixels(BADGE_HEIGHT)),
            ("shotCardWidth", pixels(CARD_WIDTH)),
            ("shotThumbHeight", pixels(THUMB_HEIGHT)),
            ("shotButtonHeight", pixels(CARD_BUTTON_HEIGHT)),
            ("shotCardGap", pixels(CARD_GAP)),
            ("durationShotCard", ms(CARD_TIME)),
            ("drainHeight", pixels(DRAIN_HEIGHT)),
            ("drainOpacity", f64::from(DRAIN_OPACITY)),
            ("countdownTick", ms(COUNTDOWN_TICK)),
        ];
        let qml = qml_properties();
        for (name, value) in ours {
            let text = qml.get(name).unwrap_or_else(|| panic!("Tokens.qml has no {name}"));
            let theirs: f64 = text.parse().unwrap_or_else(|_| panic!("Tokens.qml's {name} isn't a number: {text}"));
            assert!((theirs - value).abs() < 1e-6, "{name} is {value} here and {theirs} in Tokens.qml");
        }
    }
}
