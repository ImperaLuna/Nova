//! The power menu's own values: the ones nothing else draws with.
//! Everything shared is `nova_look::tokens` (the edge tab's corners and
//! nub, the rows' height, the draining line).
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
    (@value px $value:literal) => { px($value) };
    (@value ratio $value:literal) => { $value };
    (@value ms $value:literal) => { Duration::from_millis($value) };
}

tokens! {
    /// The menu once open, without the radius its glass reaches past the
    /// edge; the gap between its rows.
    MENU_WIDTH: px = 220.0, "powerMenuWidth";
    ROW_GAP: px = 4.0, "powerRowGap";
    /// Rows that can't run right now, and the others while one runs.
    BUSY_OPACITY: ratio = 0.35, "busyOpacity";
    /// How long an action that ends the session waits for its second
    /// click.
    DURATION_CONFIRM: ms = 4000, "durationConfirm";
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
