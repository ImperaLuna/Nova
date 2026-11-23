use gpui::{Pixels, px};
use std::time::Duration;

macro_rules! tokens {
    ($( $(#[$doc:meta])* $name:ident: $kind:ident = $value:literal; )*) => {
        $( $(#[$doc])* pub const $name: tokens!(@type $kind) = tokens!(@value $kind $value); )*
    };
    (@type px) => { Pixels };
    (@type ratio) => { f32 };
    (@type ms) => { Duration };
    (@type count) => { usize };
    (@value px $value:literal) => { px($value) };
    (@value ratio $value:literal) => { $value };
    (@value ms $value:literal) => { Duration::from_millis($value) };
    (@value count $value:literal) => { $value };
}

tokens! {
    WIDTH: px = 368.0;
    PAGE_SLIDE: px = 28.0;
    DURATION_PAGE: ms = 280;
    REVEAL_FIRST: ratio = 0.35;
    REVEAL_STAGGER: ratio = 0.14;
    REVEAL_RISE: px = 6.0;
    BADGE_SIZE: px = 15.0;
    BADGE_PAD: px = 4.0;
    BADGE_OFFSET: px = 3.0;
    BADGE_RING: px = 2.0;
    BADGE_ICON: px = 9.0;
    BADGE_MAX: count = 9;
    BADGE_QUIET_OPACITY: ratio = 0.55;
}
