//! The lock screen's own values: the ones nothing else draws with.
//! Everything shared is `nova_look::tokens`.
//!
//! Until the Quickshell shell is removed (IMP-368) the same values live in
//! `ui/theme/Tokens.qml`, most as `lock*`; a test holds the two equal.

use gpui::{Pixels, px};
use std::time::Duration;

/// Defines each token as a constant, and lists it with its `Tokens.qml`
/// property for the test that compares them.
macro_rules! tokens {
    ($( $(#[$doc:meta])* $name:ident: $kind:ident = $value:literal, $qml:literal; )*) => {
        $( $(#[$doc])* pub const $name: tokens!(@type $kind) = tokens!(@value $kind $value); )*

        #[cfg(test)]
        const QML_TOKENS: &[(&str, f64)] = &[ $( ($qml, $value as f64), )* ];
    };
    (@type px) => { Pixels };
    (@type ratio) => { f32 };
    (@type ms) => { Duration };
    (@value px $value:literal) => { px($value) };
    (@value ratio $value:literal) => { $value };
    (@value ms $value:literal) => { Duration::from_millis($value) };
}

tokens! {
    /// The circle with the user's initial, and the initial's size.
    AVATAR_SIZE: px = 72.0, "lockAvatarSize";
    INITIAL_SIZE: px = 28.0, "lockInitialSize";
    FIELD_WIDTH: px = 280.0, "lockFieldWidth";
    FIELD_HEIGHT: px = 44.0, "lockFieldHeight";
    /// A typed character's dot, and the room between two.
    DOT_SIZE: px = 8.0, "lockDotSize";
    DOT_GAP: px = 6.0, "lockDotGap";
    /// Between the avatar, the name, the field and the line under it.
    GAP: px = 16.0, "lockGap";
    /// How far a wrong password shakes the field to each side, and how many
    /// times it swings.
    SHAKE: px = 8.0, "lockShake";
    SHAKE_SWINGS: ratio = 4.0, "lockShakeSwings";
    /// The field's ring, the accent this strong.
    FOCUS_OPACITY: ratio = 0.75, "lockFocusOpacity";
    DURATION_SHAKE: ms = 360, "durationShake";
    /// The prompt fading before the lock lets go.
    DURATION_UNLOCK: ms = 400, "durationUnlock";
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKENS_QML: &str = include_str!("../../../../ui/theme/Tokens.qml");

    /// A property's text in Tokens.qml.
    fn qml(name: &str) -> &'static str {
        TOKENS_QML
            .lines()
            .filter_map(|line| line.trim().strip_prefix("readonly property ")?.split_once(' ')?.1.split_once(':'))
            .find_map(|(found, value)| (found.trim() == name).then_some(value.trim()))
            .unwrap_or_else(|| panic!("Tokens.qml has no {name}"))
    }

    #[test]
    fn every_token_has_tokens_qml_s_value() {
        for (name, value) in QML_TOKENS {
            let text = qml(name);
            let theirs: f64 = text.parse().unwrap_or_else(|_| panic!("{name} isn't a number: {text}"));
            assert!((theirs - value).abs() < 1e-6, "{name} is {value} here and {theirs} in Tokens.qml");
        }
    }
}
