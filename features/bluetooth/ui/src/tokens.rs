//! The Bluetooth page's and pop-up's own values: the ones nothing else draws with.
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
    /// A pairing question: its code in a row of cells, the field a PIN is
    /// typed in, and its card's tint (and a failure's) over the row.
    PAIRING_CELL_WIDTH: px = 28.0;
    PAIRING_CELL_HEIGHT: px = 36.0;
    PAIRING_CELL_GAP: px = 4.0;
    PAIRING_FIELD_HEIGHT: px = 34.0;
    PAIRING_TINT_OPACITY: ratio = 0.10;
    /// A row's small round buttons (⋯ and stop).
    ROUND_BUTTON: px = 26.0;
    /// The pop-up that asks while the page isn't open: as wide and round as
    /// a notification's.
    POPUP_WIDTH: px = 380.0;
    POPUP_RADIUS: px = 20.0;
}
