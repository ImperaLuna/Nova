//! The network page's own values: the ones nothing else draws with.
//! Everything shared is `nova_look::tokens`.

use gpui::{Pixels, px};

/// Defines each token as a constant of its kind.
macro_rules! tokens {
    ($( $(#[$doc:meta])* $name:ident: $kind:ident = $value:literal; )*) => {
        $( $(#[$doc])* pub const $name: tokens!(@type $kind) = tokens!(@value $kind $value); )*
    };
    (@type px) => { Pixels };
    (@type ratio) => { f32 };
    (@value px $value:literal) => { px($value) };
    (@value ratio $value:literal) => { $value };
}

tokens! {
    /// The traffic card: the graph's height, and the dot beside each
    /// speed in its line's color.
    TRAFFIC_GRAPH_HEIGHT: px = 54.0;
    LEGEND_DOT: px = 7.0;
    /// The arcs a weak signal doesn't reach, at this opacity.
    WIFI_ARC_FADED_OPACITY: ratio = 0.3;
    /// A network's ⋯ and its menu.
    MORE_SIZE: px = 24.0;
    MENU_WIDTH: px = 170.0;
    /// The password field.
    PASSWORD_HEIGHT: px = 34.0;
}
