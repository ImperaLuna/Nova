//! The resources feature's own values: the ones nothing else draws with.
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
    /// A ring on the card: this wide, its arc this thick.
    RING_SIZE: px = 52.0;
    RING_STROKE: px = 4.0;
    /// The page's graph, and the CPU's area under its line (its lines are
    /// `nova_look`'s `GRAPH_LINE`).
    GRAPH_HEIGHT: px = 72.0;
    GRAPH_AREA_OPACITY: ratio = 0.22;
    /// A legend's dot beside the graph.
    LEGEND_DOT: px = 7.0;
    /// An app's row, and each of its numbers' columns.
    PROCESS_ROW_HEIGHT: px = 30.0;
    PROCESS_NUMBER_WIDTH: px = 64.0;
    /// A row's end button (✕), its icon, and its wash while hovered or
    /// asking.
    END_BUTTON_WIDTH: px = 26.0;
    END_BUTTON_HEIGHT: px = 24.0;
    END_ICON_SIZE: px = 13.0;
    END_HOVER_OPACITY: ratio = 0.22;
    /// The row that asks before ending, and its buttons.
    CONFIRM_OPACITY: ratio = 0.14;
    /// The search field over the apps.
    SEARCH_HEIGHT: px = 34.0;
}
