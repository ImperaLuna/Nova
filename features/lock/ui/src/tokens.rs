//! The lock screen's own values: the ones nothing else draws with.
//! Everything shared is `nova_look::tokens`.

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
    (@value px $value:literal) => { px($value) };
    (@value ratio $value:literal) => { $value };
    (@value ms $value:literal) => { Duration::from_millis($value) };
}

tokens! {
    /// The circle with the user's initial, and the initial's size.
    AVATAR_SIZE: px = 72.0;
    INITIAL_SIZE: px = 28.0;
    FIELD_WIDTH: px = 280.0;
    FIELD_HEIGHT: px = 44.0;
    /// A typed character's dot, and the room between two.
    DOT_SIZE: px = 8.0;
    DOT_GAP: px = 6.0;
    /// Between the avatar, the name, the field and the line under it.
    GAP: px = 16.0;
    /// How far a wrong password shakes the field to each side, and how many
    /// times it swings.
    SHAKE: px = 8.0;
    SHAKE_SWINGS: ratio = 4.0;
    /// The field's ring, the accent this strong.
    FOCUS_OPACITY: ratio = 0.75;
    DURATION_SHAKE: ms = 360;
    /// The prompt fading before the lock lets go.
    DURATION_UNLOCK: ms = 400;
}
