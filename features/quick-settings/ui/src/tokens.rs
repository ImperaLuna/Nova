//! Quick settings' own values: the ones nothing else draws with. Everything
//! shared, like the tiles and sliders other features fill it with, is
//! `nova_look::tokens`.

use gpui::{Pixels, px};
use std::time::Duration;

/// Defines each token as a constant of its kind.
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
    /// The drawer: this wide against the screen's right edge (its pages'
    /// padding and the gap between sections are `nova_look`'s, which other
    /// features' pages keep too).
    WIDTH: px = 368.0;
    /// How far a page slides as pages swap, and how long it takes.
    PAGE_SLIDE: px = 28.0;
    DURATION_PAGE: ms = 280;
    /// Its sections fade in one after another as it opens: the first at
    /// this share of the morph, each next one this much later, each rising
    /// this far into place.
    REVEAL_FIRST: ratio = 0.35;
    REVEAL_STAGGER: ratio = 0.14;
    REVEAL_RISE: px = 6.0;
    /// The badge on its button: unread notifications (up to `BADGE_MAX`,
    /// then "9+"), or a grey crossed-out bell during do not disturb.
    BADGE_SIZE: px = 15.0;
    BADGE_PAD: px = 4.0;
    BADGE_OFFSET: px = 3.0;
    BADGE_RING: px = 2.0;
    BADGE_ICON: px = 9.0;
    BADGE_MAX: count = 9;
    BADGE_QUIET_OPACITY: ratio = 0.55;
}
