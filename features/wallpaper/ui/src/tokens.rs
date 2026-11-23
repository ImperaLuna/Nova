//! The carousel's own values: the ones nothing else draws with. Everything
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
    /// The middle card is this share of the screen's width, shaped like
    /// the screen.
    CARD_SHARE: ratio = 0.26;
    /// Its neighbours sit this many card widths out, tilted away (degrees),
    /// smaller and dimmer; the ones past them each this much further and
    /// smaller.
    NEIGHBOUR: ratio = 0.74;
    STEP: ratio = 0.27;
    TILT: ratio = 50.0;
    SIDE_SCALE: ratio = 0.86;
    DEPTH_SCALE: ratio = 0.06;
    SIDE_DIM: ratio = 0.28;
    /// This many cards show either side of the middle one.
    SIDES: count = 3;
    /// The picked card's ring.
    RING: px = 3.0;
    /// Opening, the deck rises this far into place and its cards spread
    /// out from this share of their distance.
    ENTER_RISE: px = 24.0;
    ENTER_SPREAD: ratio = 0.6;
    /// Under the deck: the picked image's name and where the pick goes.
    LABEL_GAP: px = 28.0;
    LABEL_HEIGHT: px = 40.0;
    LABEL_PAD: px = 16.0;
    CHIP_HEIGHT: px = 28.0;
    /// With more than one screen the label holds the screen picker: every
    /// screen to scale, the widest this wide, apart by this gap, the ones
    /// not picked for faded to this.
    PICKER_HEIGHT: px = 64.0;
    MINI_WIDTH: px = 52.0;
    MINI_GAP: px = 10.0;
    MINI_RADIUS: px = 4.0;
    MINI_RING: px = 2.0;
    MINI_IDLE: ratio = 0.7;
    /// The ring round the edge of the screen picked for.
    SCREEN_RING: px = 4.0;
    /// The note an empty folder shows instead.
    NOTE_WIDTH: px = 420.0;
    /// A shade from this far down the preview to its bottom, so the label
    /// reads on a bright picture, at this opacity of black.
    SHADE_FROM: ratio = 0.3;
    SHADE: ratio = 0.55;
    /// A card whose image is still loading: the ink at this opacity.
    LOADING: ratio = 0.07;
    /// The dot beside "Now" and "Picking for this screen".
    LEGEND_DOT: px = 7.0;
    /// The deck moving one card, and the preview fading to the next image.
    DURATION_STEP: ms = 280;
    DURATION_PREVIEW: ms = 250;
    /// After a pick, how long the preview stays while awww fades the new
    /// image in under it (novad's TRANSITION_SECONDS and a little).
    DURATION_HAND_OVER: ms = 1100;
}
