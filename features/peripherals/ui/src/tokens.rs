//! The peripherals feature's own values: the ones nothing else draws with.
//! Everything shared is `nova_look::tokens`.

use gpui::{Pixels, px};

/// Defines each token as a constant of its kind.
macro_rules! tokens {
    ($( $(#[$doc:meta])* $name:ident: $kind:ident = $value:literal; )*) => {
        $( $(#[$doc])* pub const $name: tokens!(@type $kind) = tokens!(@value $kind $value); )*
    };
    (@type px) => { Pixels };
    (@type count) => { usize };
    (@value px $value:literal) => { px($value) };
    (@value count $value:literal) => { $value };
}

tokens! {
    /// A device's row, and how many show before the card scrolls.
    DEVICE_ROW_HEIGHT: px = 36.0;
    DEVICE_LIST_MAX_ROWS: count = 4;
    DEVICE_ICON_SIZE: px = 17.0;
    /// How it's connected, in a small chip: its icon, height, padding and
    /// the gap between icon and label.
    DEVICE_LINK_ICON_SIZE: px = 10.0;
    DEVICE_CHIP_HEIGHT: px = 18.0;
    DEVICE_CHIP_PAD: px = 7.0;
    DEVICE_CHIP_GAP: px = 4.0;
    /// Room for "100%" (or "High"), so the batteries line up.
    DEVICE_LEVEL_WIDTH: px = 34.0;
}
