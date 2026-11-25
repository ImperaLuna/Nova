use gpui::{Pixels, px};
use std::time::Duration;

macro_rules! tokens {
    ($( $(#[$doc:meta])* $name:ident: $kind:ident = $value:literal; )*) => {
        $( $(#[$doc])* pub const $name: tokens!(@type $kind) = tokens!(@value $kind $value); )*
    };
    (@type px) => { Pixels };
    (@type ratio) => { f32 };
    (@type ms) => { Duration };
    (@value px $value:literal) => { px($value) };
    (@value ratio $value:literal) => { $value };
    (@value ms $value:literal) => { Duration::from_millis($value) };
}

tokens! {
    AVATAR_SIZE: px = 72.0;
    INITIAL_SIZE: px = 28.0;
    FIELD_WIDTH: px = 280.0;
    FIELD_HEIGHT: px = 44.0;
    DOT_SIZE: px = 8.0;
    DOT_GAP: px = 6.0;
    GAP: px = 16.0;
    SHAKE: px = 8.0;
    SHAKE_SWINGS: ratio = 4.0;
    FOCUS_OPACITY: ratio = 0.75;
    DURATION_SHAKE: ms = 360;
    DURATION_UNLOCK: ms = 400;
}
