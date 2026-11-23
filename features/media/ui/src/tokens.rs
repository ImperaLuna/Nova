//! The media feature's own values: the ones nothing else draws with.
//! Everything shared is `nova_look::tokens`.
//!
//! Until the Quickshell shell is removed (IMP-368) the same values live in
//! `ui/theme/Tokens.qml` as `media*`; a test holds the two equal.

use gpui::{Pixels, px};
use std::time::Duration;

/// Defines each token as a constant, and lists it with its `Tokens.qml`
/// property for the test that compares them.
macro_rules! tokens {
    ($( $(#[$doc:meta])* $name:ident: $kind:ident = $value:literal, $qml:literal; )*) => {
        $( $(#[$doc])* pub const $name: tokens!(@type $kind) = tokens!(@value $kind $value); )*

        #[cfg(test)]
        const QML_TOKENS: &[(&str, f64)] = &[ $( ($qml, $value as f64), )* ];
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
    ART: px = 26.0, "mediaArt";
    APP_ICON: px = 22.0, "mediaAppIcon";
    /// The icon's circle, this far from the pill's left end.
    PILL_START: px = 5.0, "mediaPillStart";
    TITLE_MAX: px = 150.0, "mediaTitleMax";
    ARTIST_MAX: px = 96.0, "mediaArtistMax";
    /// A long title fades out over this much of its end, scrolls through
    /// at this many px a second, and rests this long at each end.
    FADE: px = 18.0, "mediaFade";
    MARQUEE_SPEED: ratio = 30.0, "mediaMarqueeSpeed";
    DURATION_MARQUEE_REST: ms = 800, "durationMarqueeRest";
    /// The equalizer: its bars, each this wide and this far apart, between
    /// these heights, after this much more room; a new step this often.
    EQ_BARS: count = 4, "mediaEqBars";
    EQ_BAR_WIDTH: px = 3.0, "mediaEqBarWidth";
    EQ_GAP: px = 2.0, "mediaEqGap";
    EQ_LOW: px = 3.0, "mediaEqLow";
    EQ_HIGH: px = 12.0, "mediaEqHigh";
    EQ_LEAD: px = 2.0, "mediaEqLead";
    DURATION_EQ_STEP: ms = 125, "durationEqStep";
    /// Paused, the app's icon dims to this; after a minute the pill slides
    /// away, unless its panel is open.
    PAUSED_OPACITY: ratio = 0.6, "mediaPausedOpacity";
    DURATION_LINGER: ms = 60000, "durationMediaLinger";

    // The panel: the cover and the gap beside it, the title's line height
    // (a share of its own), the progress line (taller on hover, with a
    // knob) and the room it takes clicks in, the buttons and the chips.
    COVER: px = 76.0, "mediaCover";
    BODY_GAP: px = 14.0, "mediaBodyGap";
    TITLE_LINE: ratio = 1.2, "mediaTitleLine";
    SEEK_ROOM: px = 16.0, "mediaSeekRoom";
    TRACK: px = 4.0, "mediaTrack";
    TRACK_HOVER: px = 6.0, "mediaTrackHover";
    KNOB: px = 12.0, "mediaKnob";
    PLAY: px = 48.0, "mediaPlay";
    CONTROL: px = 40.0, "mediaControl";
    CONTROL_GAP: px = 18.0, "mediaControlGap";
    CONTROL_ICON: px = 18.0, "mediaControlIcon";
    PLAY_ICON: px = 20.0, "mediaPlayIcon";
    CHIP_HEIGHT: px = 28.0, "mediaChipHeight";
    CHIP_PAD: px = 12.0, "mediaChipPad";
    CHIP_GAP: px = 6.0, "mediaChipGap";
    CHIP_DOT: px = 5.0, "mediaChipDot";
    GO_TO_ICON: px = 18.0, "mediaGoToIcon";
    PAD_BOTTOM: px = 18.0, "mediaPadBottom";
    /// Under the header and above the buttons; between the title, artist
    /// and album; above the times; above the chips and the go-to row.
    GAP: px = 10.0, "mediaGap";
    META_GAP: px = 2.0, "mediaMetaGap";
    TIMES_GAP: px = 6.0, "mediaTimesGap";
    ROW_GAP: px = 14.0, "mediaRowGap";
    /// A button the player can't act on now (no next track).
    DISABLED_OPACITY: ratio = 0.35, "mediaDisabledOpacity";
    /// How often the open panel's time counts up, and how long the line
    /// stays where a seek put it if the player doesn't say it moved.
    DURATION_PROGRESS_STEP: ms = 1000, "durationProgressStep";
    DURATION_SEEK_HOLD: ms = 2000, "durationSeekHold";
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

#[cfg(test)]
mod tests {
    use super::*;

    const TOKENS_QML: &str = include_str!("../../../../ui/theme/Tokens.qml");

    /// A property's text in Tokens.qml.
    fn qml(name: &str) -> &'static str {
        TOKENS_QML
            .lines()
            .filter_map(|line| line.trim().strip_prefix("readonly property ")?.split_once(' ')?.1.split_once(':'))
            .find_map(|(found, value)| (found.trim() == name).then_some(value.trim()))
            .unwrap_or_else(|| panic!("Tokens.qml has no {name}"))
    }

    #[test]
    fn every_token_has_tokens_qml_s_value() {
        for (name, value) in QML_TOKENS {
            let text = qml(name);
            let theirs: f64 = text.parse().unwrap_or_else(|_| panic!("{name} isn't a number: {text}"));
            assert!((theirs - value).abs() < 1e-6, "{name} is {value} here and {theirs} in Tokens.qml");
        }
    }
}
