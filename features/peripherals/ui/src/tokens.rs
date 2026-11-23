//! The peripherals feature's own values: the ones nothing else draws with.
//! Everything shared is `nova_look::tokens`.
//!
//! Until the Quickshell shell is removed (IMP-368) the same values live in
//! `ui/theme/Tokens.qml`; a test holds the two equal.

use gpui::{Pixels, px};

/// Defines each token as a constant, and lists it with its `Tokens.qml`
/// property for the test that compares them.
macro_rules! tokens {
    ($( $(#[$doc:meta])* $name:ident: $kind:ident = $value:literal, $qml:literal; )*) => {
        $( $(#[$doc])* pub const $name: tokens!(@type $kind) = tokens!(@value $kind $value); )*

        #[cfg(test)]
        const QML_TOKENS: &[(&str, f64)] = &[ $( ($qml, $value as f64), )* ];
    };
    (@type px) => { Pixels };
    (@type count) => { usize };
    (@value px $value:literal) => { px($value) };
    (@value count $value:literal) => { $value };
}

tokens! {
    /// A device's row, and how many show before the card scrolls.
    DEVICE_ROW_HEIGHT: px = 36.0, "deviceRowHeight";
    DEVICE_LIST_MAX_ROWS: count = 4, "deviceListMaxRows";
    DEVICE_ICON_SIZE: px = 17.0, "deviceIconSize";
    /// How it's connected, in a small chip: its icon, height, padding and
    /// the gap between icon and label.
    DEVICE_LINK_ICON_SIZE: px = 10.0, "deviceLinkIconSize";
    DEVICE_CHIP_HEIGHT: px = 18.0, "deviceChipHeight";
    DEVICE_CHIP_PAD: px = 7.0, "deviceChipPad";
    DEVICE_CHIP_GAP: px = 4.0, "deviceChipGap";
    /// Room for "100%" (or "High"), so the batteries line up.
    DEVICE_LEVEL_WIDTH: px = 34.0, "deviceLevelWidth";
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
            let theirs: f64 = qml(name).parse().unwrap_or_else(|_| panic!("{name} isn't a number"));
            assert!((theirs - value).abs() < 1e-6, "{name} is {value} here and {theirs} in Tokens.qml");
        }
    }
}
