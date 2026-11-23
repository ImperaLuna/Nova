//! Quick settings' own values: the ones nothing else draws with. Everything
//! shared, like the tiles and sliders other features fill it with, is
//! `nova_look::tokens`.
//!
//! Until the Quickshell shell is removed (IMP-368) the same values live in
//! `ui/theme/Tokens.qml`; a test holds the two equal.

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
    (@type count) => { usize };
    (@value px $value:literal) => { px($value) };
    (@value ratio $value:literal) => { $value };
    (@value ms $value:literal) => { Duration::from_millis($value) };
    (@value count $value:literal) => { $value };
}

tokens! {
    /// The drawer: this wide against the screen's right edge, its pages
    /// this far in from its edges (the gap between sections is
    /// `nova_look`'s, which other features' pages keep too).
    WIDTH: px = 368.0, "quickSettingsWidth";
    PAD: px = 16.0, "quickSettingsPad";
    /// How far a page slides as pages swap, and how long it takes.
    PAGE_SLIDE: px = 28.0, "pageSlide";
    DURATION_PAGE: ms = 280, "durationPage";
    /// Its sections fade in one after another as it opens: the first at
    /// this share of the morph, each next one this much later, each rising
    /// this far into place.
    REVEAL_FIRST: ratio = 0.35, "revealFirst";
    REVEAL_STAGGER: ratio = 0.14, "revealStagger";
    REVEAL_RISE: px = 6.0, "revealRise";
    /// The badge on its button: unread notifications (up to `BADGE_MAX`,
    /// then "9+"), or a grey crossed-out bell during do not disturb.
    BADGE_SIZE: px = 15.0, "badgeSize";
    BADGE_PAD: px = 4.0, "badgePad";
    BADGE_OFFSET: px = 3.0, "badgeOffset";
    BADGE_RING: px = 2.0, "badgeRing";
    BADGE_ICON: px = 9.0, "badgeIcon";
    BADGE_MAX: count = 9, "badgeMax";
    BADGE_QUIET_OPACITY: ratio = 0.55, "badgeQuiet";
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
            // The number itself, or the opacity a color is given.
            let number = text.strip_suffix(')').and_then(|call| call.rsplit(',').next()).unwrap_or(text);
            let theirs: f64 = number.trim().parse().unwrap_or_else(|_| panic!("{name} isn't a number: {text}"));
            assert!((theirs - value).abs() < 1e-6, "{name} is {value} here and {theirs} in Tokens.qml");
        }
    }
}
