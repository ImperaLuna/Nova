//! The launcher's own values: the ones nothing else draws with. Everything
//! shared is `nova_look::tokens`.
//!
//! Until the Quickshell shell is removed (IMP-368) the same values live in
//! `ui/theme/Tokens.qml` as `launcher*`; a test holds the two equal.

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
    (@type ratio) => { f32 };
    (@type count) => { usize };
    (@value px $value:literal) => { px($value) };
    (@value ratio $value:literal) => { $value };
    (@value count $value:literal) => { $value };
}

tokens! {
    LAUNCHER_WIDTH: px = 620.0, "launcherWidth";
    /// How far down the screen its top edge is, as a share of the screen's
    /// height.
    LAUNCHER_TOP: ratio = 0.17, "launcherTop";
    LAUNCHER_PAD: px = 14.0, "launcherPad";
    LAUNCHER_GAP: px = 10.0, "launcherGap";
    SEARCH_HEIGHT: px = 44.0, "launcherSearchHeight";
    /// A heading over the pinned apps or a group of rows.
    LABEL_HEIGHT: px = 16.0, "launcherLabelHeight";
    /// A pinned app's tile, its icon, and the room for its name under it.
    TILE_SIZE: px = 64.0, "launcherTileSize";
    TILE_ICON: px = 38.0, "launcherTileIcon";
    TILE_NAME: px = 16.0, "launcherTileName";
    ROW_HEIGHT: px = 46.0, "launcherRowHeight";
    ROW_GAP: px = 2.0, "launcherRowGap";
    /// The rows the list shows before it scrolls.
    ROWS: count = 6, "launcherRows";
    /// The Apps and Files tabs in the search field.
    MODES_WIDTH: px = 132.0, "launcherModesWidth";
    /// A dot file, or one in a dot folder: its name's opacity, and its
    /// icon's.
    FILE_FAINT_OPACITY: ratio = 0.45, "launcherFileFaint";
    HIDDEN_ICON_OPACITY: ratio = 0.6, "launcherHiddenIconOpacity";
    /// The note shown when nothing matches.
    EMPTY_HEIGHT: px = 92.0, "launcherEmptyHeight";
    /// The pin button on a row.
    PIN_BUTTON_SIZE: px = 24.0, "pinButtonSize";
}

/// A folder's icon, in its own warm color in every theme: `0xRRGGBB`.
pub const FOLDER_COLOR: u32 = 0x00E8_A35A;

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
        assert_eq!(qml("launcherFolder"), "\"#E8A35A\"");
    }
}
