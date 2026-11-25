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
    CARD_SHARE: ratio = 0.26;
    NEIGHBOUR: ratio = 0.74;
    STEP: ratio = 0.27;
    TILT: ratio = 50.0;
    SIDE_SCALE: ratio = 0.86;
    DEPTH_SCALE: ratio = 0.06;
    SIDE_DIM: ratio = 0.28;
    SIDES: count = 3;
    RING: px = 3.0;
    ENTER_RISE: px = 24.0;
    ENTER_SPREAD: ratio = 0.6;
    LABEL_GAP: px = 28.0;
    LABEL_HEIGHT: px = 40.0;
    LABEL_PAD: px = 16.0;
    CHIP_HEIGHT: px = 28.0;
    PICKER_HEIGHT: px = 64.0;
    MINI_WIDTH: px = 52.0;
    MINI_GAP: px = 10.0;
    MINI_RADIUS: px = 4.0;
    MINI_RING: px = 2.0;
    MINI_IDLE: ratio = 0.7;
    SCREEN_RING: px = 4.0;
    NOTE_WIDTH: px = 420.0;
    SHADE_FROM: ratio = 0.3;
    SHADE: ratio = 0.55;
    LOADING: ratio = 0.07;
    LEGEND_DOT: px = 7.0;
    DURATION_STEP: ms = 280;
    DURATION_PREVIEW: ms = 250;
    DURATION_HAND_OVER: ms = 1100;
}
