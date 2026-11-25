use gpui::{Pixels, px};
use std::time::Duration;

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
    TRAY_VISIBLE: count = 5;
    TRAY_FADE: px = 22.0;
    TRAY_ICON_SIZE: px = 18.0;
    TRAY_DOT_SIZE: px = 7.0;
    TRAY_DOT_RING: px = 2.0;
    TRAY_STAGGER: ms = 28;
    MENU_WIDTH: px = 230.0;
    MENU_ROW_HEIGHT: px = 32.0;
    MENU_ROW_RADIUS: px = 10.0;
    MENU_CHECK_SIZE: px = 14.0;
    MENU_CHECK_RADIUS: px = 4.0;
    MENU_CHECK_ICON_SIZE: px = 11.0;
    MENU_SEPARATOR_INSET: px = 6.0;
    MENU_SEPARATOR_GAP: px = 4.0;
    MENU_GAP: px = 8.0;
    TRAY_GRID_COLUMNS: count = 8;
    TRAY_GRID_CELL: px = 36.0;
    TRAY_GRID_ICON_SIZE: px = 20.0;
    TRAY_GRID_GAP: px = 4.0;
    TRAY_GRID_MAX_ROWS: count = 3;
    REVEAL_PILL_COUNT: ratio = 0.8;
}
