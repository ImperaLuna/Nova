//! The carousel's own values: the ones nothing else draws with. Everything
//! shared is `nova_look::tokens`.
//!
//! Until the Quickshell shell is removed (IMP-368) the same values live in
//! `ui/theme/Tokens.qml` as `carousel*`; a test holds the two equal.

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
    /// The middle card is this share of the screen's width, shaped like
    /// the screen.
    CARD_SHARE: ratio = 0.26, "carouselCardShare";
    /// Its neighbours sit this many card widths out, tilted away (degrees),
    /// smaller and dimmer; the ones past them each this much further and
    /// smaller.
    NEIGHBOUR: ratio = 0.74, "carouselNeighbour";
    STEP: ratio = 0.27, "carouselStep";
    TILT: ratio = 50.0, "carouselTilt";
    SIDE_SCALE: ratio = 0.86, "carouselSideScale";
    DEPTH_SCALE: ratio = 0.06, "carouselDepthScale";
    SIDE_DIM: ratio = 0.28, "carouselSideDim";
    /// This many cards show either side of the middle one.
    SIDES: count = 3, "carouselSides";
    /// The picked card's ring.
    RING: px = 3.0, "carouselRing";
    /// Opening, the deck rises this far into place and its cards spread
    /// out from this share of their distance.
    ENTER_RISE: px = 24.0, "carouselEnterRise";
    ENTER_SPREAD: ratio = 0.6, "carouselEnterSpread";
    /// Under the deck: the picked image's name and where the pick goes.
    LABEL_GAP: px = 28.0, "carouselLabelGap";
    LABEL_HEIGHT: px = 40.0, "carouselLabelHeight";
    LABEL_PAD: px = 16.0, "carouselLabelPad";
    CHIP_HEIGHT: px = 28.0, "carouselChipHeight";
    /// With more than one screen the label holds the screen picker: every
    /// screen to scale, the widest this wide, apart by this gap, the ones
    /// not picked for faded to this.
    PICKER_HEIGHT: px = 64.0, "carouselPickerHeight";
    MINI_WIDTH: px = 52.0, "carouselMiniWidth";
    MINI_GAP: px = 10.0, "carouselMiniGap";
    MINI_RADIUS: px = 4.0, "carouselMiniRadius";
    MINI_RING: px = 2.0, "carouselMiniRing";
    MINI_IDLE: ratio = 0.7, "carouselMiniIdle";
    /// The ring round the edge of the screen picked for.
    SCREEN_RING: px = 4.0, "carouselScreenRing";
    /// The note an empty folder shows instead.
    NOTE_WIDTH: px = 420.0, "carouselNoteWidth";
    /// A shade from this far down the preview to its bottom, so the label
    /// reads on a bright picture, at this opacity of black.
    SHADE_FROM: ratio = 0.3, "carouselShadeFrom";
    SHADE: ratio = 0.55, "carouselShade";
    /// A card whose image is still loading: the ink at this opacity.
    LOADING: ratio = 0.07, "carouselLoading";
    /// The dot beside "Now" and "Picking for this screen".
    LEGEND_DOT: px = 7.0, "legendDot";
    /// The deck moving one card, and the preview fading to the next image.
    DURATION_STEP: ms = 280, "durationCarouselStep";
    DURATION_PREVIEW: ms = 250, "durationPreview";
    /// After a pick, how long the preview stays while awww fades the new
    /// image in under it (novad's TRANSITION_SECONDS and a little).
    DURATION_HAND_OVER: ms = 1100, "durationWallpaperHandOver";
}

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
            // The number itself, or the opacity a color is given.
            let number = text.strip_suffix(')').and_then(|call| call.rsplit(',').next()).unwrap_or(text);
            let theirs: f64 = number.trim().parse().unwrap_or_else(|_| panic!("{name} isn't a number: {text}"));
            assert!((theirs - value).abs() < 1e-6, "{name} is {value} here and {theirs} in Tokens.qml");
        }
        assert_eq!(qml("carouselShade"), "Qt.rgba(0, 0, 0, 0.55)");
    }
}
