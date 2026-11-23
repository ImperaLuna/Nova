//! The launcher's own values: the ones nothing else draws with. Everything
//! shared is `nova_look::tokens`.

use gpui::{Pixels, px};

/// Defines each token as a constant of its kind.
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
    /// How far down the screen its top edge is, as a share of the screen's
    /// height.
    LAUNCHER_TOP: ratio = 0.17;
    LAUNCHER_PAD: px = 14.0;
    LAUNCHER_GAP: px = 10.0;
    SEARCH_HEIGHT: px = 44.0;
    /// A heading over the pinned apps or a group of rows.
    LABEL_HEIGHT: px = 16.0;
    /// A pinned app's tile, its icon, and the room for its name under it.
    TILE_SIZE: px = 64.0;
    TILE_ICON: px = 38.0;
    TILE_NAME: px = 16.0;
    ROW_HEIGHT: px = 46.0;
    ROW_GAP: px = 2.0;
    /// The rows the list shows before it scrolls.
    ROWS: count = 6;
    /// The Apps and Files tabs in the search field.
    MODES_WIDTH: px = 132.0;
    /// A dot file, or one in a dot folder: its name's opacity, and its
    /// icon's.
    FILE_FAINT_OPACITY: ratio = 0.45;
    HIDDEN_ICON_OPACITY: ratio = 0.6;
    /// The note shown when nothing matches.
    EMPTY_HEIGHT: px = 92.0;
    /// The pin button on a row.
    PIN_BUTTON_SIZE: px = 24.0;
}

/// A folder's icon, in its own warm color in every theme: `0xRRGGBB`.
pub const FOLDER_COLOR: u32 = 0x00E8_A35A;
