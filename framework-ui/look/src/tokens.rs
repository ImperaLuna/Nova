//! Nova's design tokens: every size, opacity and timing the look is made
//! of, each defined once here and named for what it means. Colors are the
//! theme's ([`crate::Theme`]), which derives its fills from the opacities
//! here.
//!
//! These are the tokens every surface shares. One feature's own values (the
//! capsule's liquid, the launcher's rows) live in a `tokens` module of that
//! feature's ui crate, by the same rule: named, defined once.
//!
//! Until the Quickshell shell is removed (IMP-368) the same values also
//! live in `ui/theme/Tokens.qml`. The `tokens!` list below names each
//! token's property there, and a test reads that file and holds the two
//! equal, so the shells can't drift apart. Change a value in both.

use crate::Family;
use gpui::{FontWeight, Pixels, px};
use std::time::Duration;

/// Defines each token as a constant, and lists it with its `Tokens.qml`
/// property for the test that compares them.
macro_rules! tokens {
    ($( $(#[$doc:meta])* $name:ident: $kind:ident = $value:literal, $qml:literal; )*) => {
        $( $(#[$doc])* pub const $name: tokens!(@type $kind) = tokens!(@value $kind $value); )*

        /// Each token's property in `ui/theme/Tokens.qml`, and its value there.
        #[cfg(test)]
        const QML_TOKENS: &[(&str, f64)] = &[ $( ($qml, $value as f64), )* ];
    };
    (@type px) => { Pixels };
    (@type ratio) => { f32 };
    (@type ms) => { Duration };
    (@value px $value:literal) => { px($value) };
    (@value ratio $value:literal) => { $value };
    (@value ms $value:literal) => { Duration::from_millis($value) };
}

tokens! {
    // Ink: text and icons are the theme's `fg`; the dimmer levels are that
    // ink at less opacity, so over glass they take on what's behind.
    /// Secondary text.
    FG_DIM_OPACITY: ratio = 0.66, "fgDimOpacity";
    /// Between the ink and the dim level, like a label under a ring.
    FG_SOFT_OPACITY: ratio = 0.72, "fgSoft";
    /// Something that can't be used right now.
    DIM_OPACITY: ratio = 0.5, "dimOpacity";

    // Fills on glass, all the ink or the background at an opacity.
    /// A solid-looking surface: the background, nearly opaque.
    SURFACE_OPACITY: ratio = 0.75, "surfaceOpacity";
    /// Something raised off a surface, like a card or a field.
    RAISED_OPACITY: ratio = 0.09, "raisedOpacity";
    /// A hairline between things on a surface.
    OUTLINE_OPACITY: ratio = 0.08, "outlineOpacity";
    /// The hover wash on things inside glass.
    HOVER_OPACITY: ratio = 0.06, "hover";
    /// The selected row of a list.
    SELECTED_OPACITY: ratio = 0.18, "selected";
    /// What's selected where the focus isn't: a quiet fill, not the accent.
    SELECTED_QUIET_OPACITY: ratio = 0.14, "selectedQuiet";
    /// The accent as a fill behind something that is running or picked,
    /// like a row the launcher is starting.
    ACCENT_FILL_OPACITY: ratio = 0.92, "accentFillOpacity";
    /// A meter's empty track, and its neutral fill.
    TRACK_OPACITY: ratio = 0.14, "track";
    METER_OPACITY: ratio = 0.85, "meter";

    /// How much of glass is its tint; the rest is the compositor's blur of
    /// what's behind.
    GLASS_OPACITY: ratio = 0.55, "glassOpacity";
    // Glass's edge (`crate::glass`): a dark outline outside, a faint light
    // line inside it, and a highlight along the top that fades out before
    // the corners.
    /// The outline, in black.
    GLASS_OUTLINE_OPACITY: ratio = 0.45, "glassOutline";
    GLASS_INNER_LINE_OPACITY: ratio = 0.06, "glassInnerLine";
    GLASS_HIGHLIGHT_OPACITY: ratio = 0.22, "glassHighlight";
    /// Where the highlight starts, as a share of the corner radius.
    GLASS_HIGHLIGHT_INSET: ratio = 0.7, "glassHighlightInset";
    /// The share of the highlight's length over which each end fades.
    GLASS_HIGHLIGHT_FADE: ratio = 0.15, "glassHighlightFade";
    GLASS_EDGE_WIDTH: px = 1.0, "glassEdgeWidth";
    /// A hairline around things inside glass.
    GLASS_RIM_OPACITY: ratio = 0.10, "glassRim";
    /// The soft shadow under glass, in black, and how far it reaches.
    SHADOW_OPACITY: ratio = 0.35, "shadow";
    SHADOW_BLUR: px = 30.0, "shadowBlur";
    SHADOW_OFFSET: px = 10.0, "shadowOffset";

    // Radii
    RADIUS_PANEL: px = 24.0, "radiusPanel";
    RADIUS_CARD: px = 16.0, "radiusCard";
    RADIUS_CONTROL: px = 12.0, "radiusControl";
    RADIUS_ROW: px = 14.0, "radiusRow";
    RADIUS_SEGMENT: px = 10.0, "radiusSegment";

    // Spacing
    GAP: px = 8.0, "gap";
    PAD: px = 12.0, "pad";
    BAR_HEIGHT: px = 36.0, "barHeight";
    /// The bar's pills: this far in from the screen's sides.
    BAR_MARGIN: px = 8.0, "barMargin";
    /// And this far from the screen's top edge and the windows below.
    BAR_GAP: px = 4.0, "barGap";
    /// The line between two groups in a pill.
    DIVIDER_HEIGHT: px = 16.0, "dividerHeight";
    DIVIDER_WIDTH: px = 1.0, "dividerWidth";
    /// Small marks, like a "wants attention" dot.
    DOT_SIZE: px = 4.0, "dotSize";
    /// The central pill's workspace numbers, each a round cell this big and
    /// this far apart, with extra room after the special workspaces. The
    /// search button beside them is a cell too.
    WORKSPACE_SIZE: px = 26.0, "workspaceSize";
    WORKSPACE_GAP: px = 2.0, "workspaceGap";
    WORKSPACE_GROUP_GAP: px = 4.0, "workspaceGroupGap";
    /// Marks inside a round cell (a ring, a dot) keep this far inside its
    /// edge.
    RING_INSET: px = 2.0, "ringInset";
    /// While the bar steps aside for a fullscreen window: the strip along
    /// the top edge that brings it back when the pointer touches it.
    REVEAL_STRIP: px = 2.0, "revealStrip";
    /// A label beside an icon sits this far right, to line up optically
    /// with the icon's edge.
    LABEL_NUDGE: px = 1.0, "labelNudge";

    // Icons: in the bar's pills, in panels, and in menus.
    ICON_SIZE_SMALL: px = 14.0, "iconSizeSmall";
    ICON_SIZE: px = 16.0, "iconSize";
    ICON_SIZE_MENU: px = 18.0, "iconSizeMenu";
    /// Between an icon and the label or number beside it.
    ICON_GAP: px = 6.0, "iconGap";
    /// A line icon's stroke, on its 24-unit grid.
    ICON_STROKE: ratio = 1.8, "iconStroke";
    /// Thin strokes, like a ring or a spinner's arc.
    STROKE_THIN: px = 1.5, "strokeThin";
    /// A loading spinner's arc, in degrees of its ring.
    SPINNER_SWEEP: ratio = 86.0, "spinnerSweep";

    // Panels: width, inner padding, the gap between sections, list rows
    // and meters.
    PANEL_WIDTH: px = 300.0, "panelWidth";
    PANEL_PAD: px = 20.0, "panelPad";
    PANEL_PAD_BOTTOM: px = 16.0, "panelPadBottom";
    SECTION_GAP: px = 18.0, "sectionGap";
    ROW_HEIGHT: px = 40.0, "rowHeight";
    /// Inner padding of a list row.
    ROW_INSET: px = 10.0, "rowInset";
    METER_HEIGHT: px = 8.0, "meterHeight";
    METER_HEIGHT_SMALL: px = 4.0, "meterHeightSmall";
    /// Small labels in a pill shape, like a plan name.
    CHIP_HEIGHT: px = 22.0, "chipHeight";
    CHIP_PAD: px = 10.0, "chipPad";
    /// A row of tabs (`crate::Segments`): its padding, also between two
    /// tabs, and a tab's height.
    SEGMENT_PAD: px = 4.0, "segmentPad";
    SEGMENT_HEIGHT: px = 30.0, "segmentHeight";
    /// The text cursor in a field (`crate::TextField`).
    CARET_WIDTH: px = 2.0, "caretWidth";
    /// The line under text an input method is still composing.
    COMPOSING_UNDERLINE: px = 1.0, "composingUnderline";
    /// An app's icon in a row (`crate::AppIcon`). Without an image it is
    /// the app's letter on a swatch, this round at this size, the letter
    /// this share of the tile.
    APP_ICON_SIZE: px = 30.0, "launcherRowIcon";
    LETTER_TILE_RADIUS: px = 9.0, "letterTileRadius";
    LETTER_SCALE: ratio = 0.47, "letterScale";
    /// The round buttons inside a pill: this far from its ends and each
    /// other.
    PILL_BUTTON_SIZE: px = 28.0, "pillButtonSize";
    PILL_BUTTON_INSET: px = 4.0, "pillButtonInset";
    PILL_BUTTON_GAP: px = 2.0, "pillButtonGap";
    /// A list row with a second line under its name (`crate::ListRow`).
    LIST_ROW_HEIGHT: px = 46.0, "powerRowHeight";
    /// A tab against the right screen edge, halfway down (the power
    /// menu): its corners, the concave corners that join it to the edge,
    /// and the nub it grows out of.
    EDGE_TAB_RADIUS: px = 27.0, "edgeTabRadius";
    EDGE_TAB_FILLET: px = 22.0, "edgeTabFillet";
    EDGE_TAB_NUB: px = 40.0, "edgeTabNub";
    /// The line that drains along the bottom of what waits for a second
    /// click (a power menu row, a screenshot's card), on the accent.
    DRAIN_HEIGHT: px = 3.0, "drainHeight";
    DRAIN_OPACITY: ratio = 0.55, "drainOpacity";
    /// The padding inside a menu and a card that holds a list (a tray
    /// app's menu, the Devices and Tray cards).
    MENU_PAD: px = 6.0, "menuPad";

    // Quick settings' controls, which other features fill it with.
    /// Its header's logo, and how tall every page's top row is
    /// (`crate::PageHeader`).
    LOGO_SIZE: px = 36.0, "logoSize";
    /// Between the sections of its pages, its own and other features'.
    QUICK_SETTINGS_GAP: px = 12.0, "quickSettingsGap";
    /// How far quick settings' pages sit in from its edges: a page's top
    /// row leaves room for the pill's buttons by it
    /// (`page_header::header_inset`).
    QUICK_SETTINGS_PAD: px = 16.0, "quickSettingsPad";
    /// A toggle tile (`crate::Tile`), and the arrow at its end that opens
    /// its page.
    TILE_HEIGHT: px = 58.0, "tileHeight";
    TILE_ARROW_WIDTH: px = 22.0, "tileArrowWidth";
    /// A thick slider (`crate::Slider`): its round icon button beside the
    /// track, the gap between, the fill (the ink at this opacity, less
    /// while muted) and the button after it that opens a page.
    SLIDER_HEIGHT: px = 34.0, "sliderHeight";
    SLIDER_GAP: px = 6.0, "sliderGap";
    SLIDER_FILL_OPACITY: ratio = 0.22, "sliderFill";
    SLIDER_MUTED_OPACITY: ratio = 0.3, "sliderMutedOpacity";
    SLIDER_BUTTON_WIDTH: px = 30.0, "sliderButtonWidth";
    /// An app's slider shows the app's icon, its corners less round, and
    /// fainter while muted.
    SLIDER_APP_RADIUS: px = 11.0, "appIconRadius";
    SLIDER_APP_MUTED_OPACITY: ratio = 0.45, "appMutedOpacity";
    // Small controls on quick settings' pages: an on/off switch, the
    // buttons on a card that asks something, a device's battery (a rim with
    // a nub, filled to its level, with a bolt while charging), and the
    // lines of a graph over time.
    SWITCH_WIDTH: px = 40.0, "switchWidth";
    SWITCH_HEIGHT: px = 24.0, "switchHeight";
    SWITCH_KNOB: px = 18.0, "switchKnob";
    CONFIRM_BUTTON_HEIGHT: px = 26.0, "confirmButtonHeight";
    DEVICE_BATTERY_WIDTH: px = 22.0, "deviceBatteryWidth";
    DEVICE_BATTERY_HEIGHT: px = 11.0, "deviceBatteryHeight";
    DEVICE_BATTERY_RADIUS: px = 3.0, "deviceBatteryRadius";
    DEVICE_BATTERY_INSET: px = 2.0, "deviceBatteryInset";
    DEVICE_BATTERY_NUB_WIDTH: px = 2.0, "deviceBatteryNubWidth";
    DEVICE_BATTERY_NUB_HEIGHT: px = 4.0, "deviceBatteryNubHeight";
    DEVICE_BATTERY_RIM: px = 1.25, "deviceBatteryRim";
    DEVICE_BATTERY_RIM_OPACITY: ratio = 0.55, "deviceBatteryRimOpacity";
    DEVICE_BOLT_SIZE: px = 9.0, "deviceBoltSize";
    GRAPH_LINE: px = 2.0, "graphLine";

    // The type scale: six steps, from labels up, then a panel's headline.
    /// Small caps labels and badges.
    FONT_SIZE_MICRO: px = 10.0, "fontSizeMicro";
    /// Secondary text: notification bodies, subtitles, times.
    FONT_SIZE_SMALL: px = 12.0, "fontSizeSmall";
    /// Body text: rows, names, the central pill's time.
    FONT_SIZE: px = 13.0, "fontSize";
    /// A page's title beside its back button, and the search field.
    FONT_SIZE_HEADING: px = 15.0, "fontSizeHeading";
    /// A name or number on its own.
    FONT_SIZE_TITLE: px = 18.0, "fontSizeTitle";
    /// Big words: a greeting, an empty list.
    FONT_SIZE_DISPLAY: px = 24.0, "fontSizeDisplay";
    /// A panel's headline number.
    FONT_SIZE_HERO: px = 56.0, "fontSizeHero";
    LINE_HEIGHT_HERO: px = 60.0, "lineHeightHero";
    /// Line height for text that wraps, as a share of its size.
    LINE_HEIGHT_WRAPPED: ratio = 1.5, "lineHeightWrapped";

    // Motion
    DURATION_FAST: ms = 120, "durationFast";
    /// A panel growing out of its pill, and back.
    DURATION_PANEL: ms = 250, "durationPanel";
    /// Content fading in once a panel has grown.
    DURATION_FADE: ms = 140, "durationFade";
    /// Meters filling.
    DURATION_FILL: ms = 600, "durationFill";
    DURATION_COLOR: ms = 200, "durationColor";
    /// Something changing size as its content changes.
    DURATION_RESIZE: ms = 180, "durationResize";
    /// The bar stays this long after the pointer leaves it over a
    /// fullscreen window.
    DURATION_PEEK_LINGER: ms = 400, "durationPeekLinger";
    /// One turn of a loading spinner.
    DURATION_SPIN: ms = 1000, "durationSpin";
    /// When a panel's content starts to show, as a share of its morph, and
    /// when its meters and rings start to fill.
    REVEAL_CONTENT: ratio = 0.5, "revealContent";
    REVEAL_FILL: ratio = 0.6, "revealFill";
    /// The scale something new grows in from.
    ENTER_SCALE: ratio = 0.6, "enterScale";
}

/// The room a glass's shadow needs around it.
pub const SHADOW_ROOM: Pixels = px(40.0);

/// How far the central pill's round cells (its search button, the
/// workspace numbers) sit from the pill's ends: as far as from its top,
/// `(BAR_HEIGHT - WORKSPACE_SIZE) / 2` (a test holds it so).
pub const WORKSPACE_INSET: Pixels = px(5.0);

/// A dashed ring's dashes and the gaps between them, as long as each other
/// (`ringDashes` in Tokens.qml, in Qt's units of the stroke's width).
pub const RING_DASH: f32 = 2.0;

/// The grid line icons are drawn on.
pub const ICON_GRID: f32 = 24.0;

/// The cubic Bézier every panel motion runs on (fast out, gentle stop), as
/// CSS writes it: x1, y1, x2, y2.
pub const EASING: [f32; 4] = [0.2, 0.8, 0.2, 1.0];

// Type: the families, and the weights of running text, of labels and
// names, of titles and picked options, and of a badge's count. A family
// draws with the face this system matches it to, as Qt's does ([`Family`]).
pub const FONT_SANS: Family = Family::named("Inter");
pub const FONT_MONO: Family = Family::named("JetBrains Mono");
pub const FONT_DISPLAY: Family = Family::named("Instrument Serif");
/// A line's height as a share of its text size: Inter's own spacing, which
/// is how Qt lays text out (GPUI's default is 1.618). `body_text` sets it,
/// so nothing else does.
pub const LINE_HEIGHT: f32 = 1.21;
pub const WEIGHT_BODY: FontWeight = FontWeight::NORMAL;
pub const WEIGHT_LABEL: FontWeight = FontWeight::MEDIUM;
pub const WEIGHT_TITLE: FontWeight = FontWeight::SEMIBOLD;
pub const WEIGHT_BADGE: FontWeight = FontWeight::BOLD;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const TOKENS_QML: &str = include_str!("../../../ui/theme/Tokens.qml");

    /// The text of each `readonly property` in Tokens.qml, by name.
    fn qml_properties() -> HashMap<&'static str, &'static str> {
        TOKENS_QML
            .lines()
            .filter_map(|line| {
                let mut words = line.trim().strip_prefix("readonly property ")?.splitn(2, ' ');
                let _kind = words.next()?;
                let (name, value) = words.next()?.split_once(':')?;
                Some((name.trim(), value.trim()))
            })
            .collect()
    }

    /// A property's number: the value itself, or the opacity a color is
    /// given (`alpha(fg, 0.06)`, `Qt.rgba(0, 0, 0, 0.45)`).
    fn number(value: &str) -> Option<f64> {
        let opacity = value.strip_suffix(')').and_then(|call| call.rsplit(',').next());
        opacity.unwrap_or(value).trim().parse().ok()
    }

    #[test]
    fn every_shared_token_has_tokens_qml_s_value() {
        let qml = qml_properties();
        for (name, value) in QML_TOKENS {
            let text = qml.get(name).unwrap_or_else(|| panic!("Tokens.qml has no {name}"));
            let theirs = number(text).unwrap_or_else(|| panic!("Tokens.qml's {name} isn't a number: {text}"));
            assert!((theirs - value).abs() < 1e-6, "{name} is {value} here and {theirs} in Tokens.qml");
        }
    }

    #[test]
    fn the_values_tokens_qml_writes_another_way_match_too() {
        let qml = qml_properties();
        assert_eq!(qml["easing"], "[0.2, 0.8, 0.2, 1, 1, 1]", "EASING is its first four numbers");
        assert_eq!(EASING, [0.2, 0.8, 0.2, 1.0]);
        assert_eq!(qml["ringDashes"], format!("[{RING_DASH}, {RING_DASH}]"));
        assert_eq!(qml["shadowRoom"], "shadowBlur + shadowOffset");
        assert_eq!(SHADOW_ROOM, SHADOW_BLUR + SHADOW_OFFSET);
        // CentralPill.qml's numbersInset.
        assert_eq!(WORKSPACE_INSET, (BAR_HEIGHT - WORKSPACE_SIZE) / 2.0);
        assert_eq!(qml["fontSans"], format!("{FONT_SANS:?}"));
        assert_eq!(qml["fontMono"], format!("{FONT_MONO:?}"));
        assert_eq!(qml["fontDisplay"], format!("{FONT_DISPLAY:?}"));
        for (name, qt_weight, weight) in [
            ("weightBody", "Font.Normal", WEIGHT_BODY),
            ("weightLabel", "Font.Medium", WEIGHT_LABEL),
            ("weightTitle", "Font.DemiBold", WEIGHT_TITLE),
            ("weightBadge", "Font.Bold", WEIGHT_BADGE),
        ] {
            assert_eq!(qml[name], qt_weight);
            let expected = match qt_weight {
                "Font.Normal" => 400.0,
                "Font.Medium" => 500.0,
                "Font.DemiBold" => 600.0,
                _ => 700.0,
            };
            assert_eq!(weight.0, expected, "{name}");
        }
    }
}
