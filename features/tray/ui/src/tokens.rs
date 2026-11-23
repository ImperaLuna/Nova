//! The tray's own values: the ones nothing else draws with. Everything
//! shared is `nova_look::tokens`.
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
    (@type count) => { usize };
    (@type ms) => { Duration };
    (@value px $value:literal) => { px($value) };
    (@value ratio $value:literal) => { $value };
    (@value count $value:literal) => { $value };
    (@value ms $value:literal) => { Duration::from_millis($value) };
}

tokens! {
    /// The icons the strip shows at most; the rest scroll.
    TRAY_VISIBLE: count = 5, "trayVisible";
    /// The strip's ends fade over this width where more icons are out of sight.
    TRAY_FADE: px = 22.0, "trayFade";
    TRAY_ICON_SIZE: px = 18.0, "trayIconSize";
    /// The dot of an app asking for attention, ringed in the bar's color.
    TRAY_DOT_SIZE: px = 7.0, "trayDotSize";
    TRAY_DOT_RING: px = 2.0, "trayDotRing";
    /// How much later each icon pops in after the one before it.
    TRAY_STAGGER: ms = 28, "trayStagger";
    /// An app's menu: this wide, its rows and corners (its padding is
    /// `nova_look`'s `MENU_PAD`).
    MENU_WIDTH: px = 230.0, "menuWidth";
    MENU_ROW_HEIGHT: px = 32.0, "menuRowHeight";
    MENU_ROW_RADIUS: px = 10.0, "menuRowRadius";
    MENU_CHECK_SIZE: px = 14.0, "menuCheckSize";
    MENU_CHECK_RADIUS: px = 4.0, "menuCheckRadius";
    MENU_CHECK_ICON_SIZE: px = 11.0, "menuCheckIconSize";
    MENU_SEPARATOR_INSET: px = 6.0, "menuSeparatorInset";
    MENU_SEPARATOR_GAP: px = 4.0, "menuSeparatorGap";
    /// Between the bar and the menu, and how far past its icon it reaches.
    MENU_GAP: px = 8.0, "menuGap";
    /// Quick settings' Tray card: its grid of every app, this many to a
    /// row, each cell this tall with its icon this big, and this many rows
    /// before it scrolls.
    TRAY_GRID_COLUMNS: count = 8, "trayGridColumns";
    TRAY_GRID_CELL: px = 36.0, "trayGridCell";
    TRAY_GRID_ICON_SIZE: px = 20.0, "trayGridIconSize";
    TRAY_GRID_GAP: px = 4.0, "trayGridGap";
    TRAY_GRID_MAX_ROWS: count = 3, "trayGridMaxRows";
    /// The "+N" counts show once the pill has grown this far.
    REVEAL_PILL_COUNT: ratio = 0.8, "revealPillCount";
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
