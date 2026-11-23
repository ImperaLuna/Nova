//! The tray's own values: the ones nothing else draws with. Everything
//! shared is `nova_look::tokens`.

use gpui::{Pixels, px};
use std::time::Duration;

/// Defines each token as a constant of its kind.
macro_rules! tokens {
    ($( $(#[$doc:meta])* $name:ident: $kind:ident = $value:literal; )*) => {
        $( $(#[$doc])* pub const $name: tokens!(@type $kind) = tokens!(@value $kind $value); )*
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
    TRAY_VISIBLE: count = 5;
    /// The strip's ends fade over this width where more icons are out of sight.
    TRAY_FADE: px = 22.0;
    TRAY_ICON_SIZE: px = 18.0;
    /// The dot of an app asking for attention, ringed in the bar's color.
    TRAY_DOT_SIZE: px = 7.0;
    TRAY_DOT_RING: px = 2.0;
    /// How much later each icon pops in after the one before it.
    TRAY_STAGGER: ms = 28;
    /// An app's menu: this wide, its rows and corners (its padding is
    /// `nova_look`'s `MENU_PAD`).
    MENU_WIDTH: px = 230.0;
    MENU_ROW_HEIGHT: px = 32.0;
    MENU_ROW_RADIUS: px = 10.0;
    MENU_CHECK_SIZE: px = 14.0;
    MENU_CHECK_RADIUS: px = 4.0;
    MENU_CHECK_ICON_SIZE: px = 11.0;
    MENU_SEPARATOR_INSET: px = 6.0;
    MENU_SEPARATOR_GAP: px = 4.0;
    /// Between the bar and the menu, and how far past its icon it reaches.
    MENU_GAP: px = 8.0;
    /// Quick settings' Tray card: its grid of every app, this many to a
    /// row, each cell this tall with its icon this big, and this many rows
    /// before it scrolls.
    TRAY_GRID_COLUMNS: count = 8;
    TRAY_GRID_CELL: px = 36.0;
    TRAY_GRID_ICON_SIZE: px = 20.0;
    TRAY_GRID_GAP: px = 4.0;
    TRAY_GRID_MAX_ROWS: count = 3;
    /// The "+N" counts show once the pill has grown this far.
    REVEAL_PILL_COUNT: ratio = 0.8;
}
