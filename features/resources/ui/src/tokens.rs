//! The resources feature's own values: the ones nothing else draws with.
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
    (@type ratio) => { f32 };
    (@value px $value:literal) => { px($value) };
    (@value ratio $value:literal) => { $value };
}

tokens! {
    /// A ring on the card: this wide, its arc this thick.
    RING_SIZE: px = 52.0, "ringSize";
    RING_STROKE: px = 4.0, "ringStroke";
    /// The page's graph, and the CPU's area under its line (its lines are
    /// `nova_look`'s `GRAPH_LINE`).
    GRAPH_HEIGHT: px = 72.0, "graphHeight";
    GRAPH_AREA_OPACITY: ratio = 0.22, "graphAreaOpacity";
    /// A legend's dot beside the graph.
    LEGEND_DOT: px = 7.0, "legendDot";
    /// An app's row, and each of its numbers' columns.
    PROCESS_ROW_HEIGHT: px = 30.0, "processRowHeight";
    PROCESS_NUMBER_WIDTH: px = 64.0, "processNumberWidth";
    /// A row's end button (✕), its icon, and its wash while hovered or
    /// asking.
    END_BUTTON_WIDTH: px = 26.0, "endButtonWidth";
    END_BUTTON_HEIGHT: px = 24.0, "endButtonHeight";
    END_ICON_SIZE: px = 13.0, "endIconSize";
    END_HOVER_OPACITY: ratio = 0.22, "endHoverOpacity";
    /// The row that asks before ending, and its buttons.
    CONFIRM_OPACITY: ratio = 0.14, "confirmOpacity";
    CONFIRM_BUTTON_HEIGHT: px = 26.0, "confirmButtonHeight";
    /// The search field over the apps.
    SEARCH_HEIGHT: px = 34.0, "searchHeight";
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
