//! The power menu's own values: the ones nothing else draws with.
//! Everything shared is `nova_look::tokens` (the edge tab's corners and
//! nub, the rows' height, the draining line).

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
    /// The menu once open, without the radius its glass reaches past the
    /// edge; the gap between its rows.
    MENU_WIDTH: px = 220.0;
    ROW_GAP: px = 4.0;
    /// Rows that can't run right now, and the others while one runs.
    BUSY_OPACITY: ratio = 0.35;
    /// How long an action that ends the session waits for its second
    /// click.
    DURATION_CONFIRM: ms = 4000;
}
