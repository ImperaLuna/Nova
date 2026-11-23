//! The network page's own values: the ones nothing else draws with.
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
    /// The traffic card: the graph's height, and the dot beside each
    /// speed in its line's color.
    TRAFFIC_GRAPH_HEIGHT: px = 54.0, "trafficGraphHeight";
    LEGEND_DOT: px = 7.0, "legendDot";
    /// The arcs a weak signal doesn't reach, at this opacity.
    WIFI_ARC_FADED_OPACITY: ratio = 0.3, "wifiArcFadedOpacity";
    /// A network's ⋯ and its menu.
    MORE_SIZE: px = 24.0, "notificationCloseSize";
    MENU_WIDTH: px = 170.0, "networkMenuWidth";
    /// The password field.
    PASSWORD_HEIGHT: px = 34.0, "searchHeight";
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
