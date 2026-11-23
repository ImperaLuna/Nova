//! The media feature's own values: the ones nothing else draws with.
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
    (@type count) => { usize };
    (@type ms) => { Duration };
    (@value px $value:literal) => { px($value) };
    (@value ratio $value:literal) => { $value };
    (@value count $value:literal) => { $value };
    (@value ms $value:literal) => { Duration::from_millis($value) };
}

tokens! {
    // The pill: the app's icon in a circle (a workspace number's size), the
    // title and the artist cut to these widths.
    ART: px = 26.0;
    APP_ICON: px = 22.0;
    /// The icon's circle, this far from the pill's left end.
    PILL_START: px = 5.0;
    TITLE_MAX: px = 150.0;
    ARTIST_MAX: px = 96.0;
    /// A long title fades out over this much of its end, scrolls through
    /// at this many px a second, and rests this long at each end.
    FADE: px = 18.0;
    MARQUEE_SPEED: ratio = 30.0;
    DURATION_MARQUEE_REST: ms = 800;
    /// The equalizer: its bars, each this wide and this far apart, between
    /// these heights, after this much more room; a new step this often.
    EQ_BARS: count = 4;
    EQ_BAR_WIDTH: px = 3.0;
    EQ_GAP: px = 2.0;
    EQ_LOW: px = 3.0;
    EQ_HIGH: px = 12.0;
    EQ_LEAD: px = 2.0;
    DURATION_EQ_STEP: ms = 125;
    /// Paused, the app's icon dims to this; after a minute the pill slides
    /// away, unless its panel is open.
    PAUSED_OPACITY: ratio = 0.6;
    DURATION_LINGER: ms = 60000;

    // The panel: the cover and the gap beside it, the title's line height
    // (a share of its own), the progress line (taller on hover, with a
    // knob) and the room it takes clicks in, the buttons and the chips.
    COVER: px = 76.0;
    BODY_GAP: px = 14.0;
    TITLE_LINE: ratio = 1.2;
    SEEK_ROOM: px = 16.0;
    TRACK: px = 4.0;
    TRACK_HOVER: px = 6.0;
    KNOB: px = 12.0;
    PLAY: px = 48.0;
    CONTROL: px = 40.0;
    CONTROL_GAP: px = 18.0;
    CONTROL_ICON: px = 18.0;
    PLAY_ICON: px = 20.0;
    CHIP_HEIGHT: px = 28.0;
    CHIP_PAD: px = 12.0;
    CHIP_GAP: px = 6.0;
    CHIP_DOT: px = 5.0;
    GO_TO_ICON: px = 18.0;
    PAD_BOTTOM: px = 18.0;
    /// Under the header and above the buttons; between the title, artist
    /// and album; above the times; above the chips and the go-to row.
    GAP: px = 10.0;
    META_GAP: px = 2.0;
    TIMES_GAP: px = 6.0;
    ROW_GAP: px = 14.0;
    /// A button the player can't act on now (no next track).
    DISABLED_OPACITY: ratio = 0.35;
    /// How often the open panel's time counts up, and how long the line
    /// stays where a seek put it if the player doesn't say it moved.
    DURATION_PROGRESS_STEP: ms = 1000;
    DURATION_SEEK_HOLD: ms = 2000;
}

/// The equalizer between steps (GPUI only: today's shell jumps straight
/// to each step). Its bars close in on a new height with these time
/// constants, quick going up and slower coming down, and its outline has
/// this many points between two bars. It draws at most this many frames a
/// second, whatever the screen's rate: each one redraws the whole stage.
pub const DURATION_EQ_RISE: Duration = Duration::from_millis(40);
pub const DURATION_EQ_FALL: Duration = Duration::from_millis(160);
pub const EQ_OUTLINE_SAMPLES: usize = 6;
pub const EQ_FRAMES_PER_SECOND: u32 = 30;
