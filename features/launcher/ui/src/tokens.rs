use gpui::{Pixels, px};

macro_rules! tokens {
    ($( $(#[$doc:meta])* $name:ident: $kind:ident = $value:literal; )*) => {
        $( $(#[$doc])* pub const $name: tokens!(@type $kind) = tokens!(@value $kind $value); )*
    };
    (@type px) => { Pixels };
    (@type ratio) => { f32 };
    (@type count) => { usize };
    (@value px $value:literal) => { px($value) };
    (@value ratio $value:literal) => { $value };
    (@value count $value:literal) => { $value };
}

tokens! {
    LAUNCHER_WIDTH: px = 620.0;
    LAUNCHER_TOP: ratio = 0.17;
    LAUNCHER_PAD: px = 14.0;
    LAUNCHER_GAP: px = 10.0;
    SEARCH_HEIGHT: px = 44.0;
    LABEL_HEIGHT: px = 16.0;
    TILE_SIZE: px = 64.0;
    TILE_ICON: px = 38.0;
    TILE_NAME: px = 16.0;
    ROW_HEIGHT: px = 46.0;
    ROW_GAP: px = 2.0;
    ROWS: count = 6;
    MODES_WIDTH: px = 132.0;
    FILE_FAINT_OPACITY: ratio = 0.45;
    HIDDEN_ICON_OPACITY: ratio = 0.6;
    EMPTY_HEIGHT: px = 92.0;
    PIN_BUTTON_SIZE: px = 24.0;
}

pub const FOLDER_COLOR: u32 = 0x00E8_A35A;
