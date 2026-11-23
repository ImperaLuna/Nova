//! Nova's design tokens: every size, opacity and timing the look is made
//! of, each defined once here and named for what it means. Colors are the
//! theme's ([`crate::Theme`]), which derives its fills from the opacities
//! here.
//!
//! These are the tokens every surface shares. One feature's own values (the
//! capsule's liquid, the launcher's rows) live in a `tokens` module of that
//! feature's ui crate, by the same rule: named, defined once.

use crate::Family;
use gpui::{FontWeight, Pixels, px};
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
    // Ink: text and icons are the theme's `fg`; the dimmer levels are that
    // ink at less opacity, so over glass they take on what's behind.
    /// Secondary text.
    FG_DIM_OPACITY: ratio = 0.66;
    /// Between the ink and the dim level, like a label under a ring.
    FG_SOFT_OPACITY: ratio = 0.72;
    /// Something that can't be used right now.
    DIM_OPACITY: ratio = 0.5;

    // Fills on glass, all the ink or the background at an opacity.
    /// A solid-looking surface: the background, nearly opaque.
    SURFACE_OPACITY: ratio = 0.75;
    /// Something raised off a surface, like a card or a field.
    RAISED_OPACITY: ratio = 0.09;
    /// A hairline between things on a surface.
    OUTLINE_OPACITY: ratio = 0.08;
    /// The hover wash on things inside glass.
    HOVER_OPACITY: ratio = 0.06;
    /// The selected row of a list.
    SELECTED_OPACITY: ratio = 0.18;
    /// What's selected where the focus isn't: a quiet fill, not the accent.
    SELECTED_QUIET_OPACITY: ratio = 0.14;
    /// The accent as a fill behind something that is running or picked,
    /// like a row the launcher is starting.
    ACCENT_FILL_OPACITY: ratio = 0.92;
    /// A meter's empty track, and its neutral fill.
    TRACK_OPACITY: ratio = 0.14;
    METER_OPACITY: ratio = 0.85;

    /// How much of glass is its tint; the rest is the compositor's blur of
    /// what's behind.
    GLASS_OPACITY: ratio = 0.55;
    // Glass's edge (`crate::glass`): a dark outline outside, a faint light
    // line inside it, and a highlight along the top that fades out before
    // the corners.
    /// The outline, in black.
    GLASS_OUTLINE_OPACITY: ratio = 0.45;
    GLASS_INNER_LINE_OPACITY: ratio = 0.06;
    GLASS_HIGHLIGHT_OPACITY: ratio = 0.22;
    /// Where the highlight starts, as a share of the corner radius.
    GLASS_HIGHLIGHT_INSET: ratio = 0.7;
    /// The share of the highlight's length over which each end fades.
    GLASS_HIGHLIGHT_FADE: ratio = 0.15;
    GLASS_EDGE_WIDTH: px = 1.0;
    /// A hairline around things inside glass.
    GLASS_RIM_OPACITY: ratio = 0.10;
    /// The soft shadow under glass, in black, and how far it reaches.
    SHADOW_OPACITY: ratio = 0.35;
    SHADOW_BLUR: px = 30.0;
    SHADOW_OFFSET: px = 10.0;

    // Radii
    RADIUS_PANEL: px = 24.0;
    RADIUS_CARD: px = 16.0;
    RADIUS_CONTROL: px = 12.0;
    RADIUS_ROW: px = 14.0;
    RADIUS_SEGMENT: px = 10.0;

    // Spacing
    GAP: px = 8.0;
    PAD: px = 12.0;
    BAR_HEIGHT: px = 36.0;
    /// The bar's pills: this far in from the screen's sides.
    BAR_MARGIN: px = 8.0;
    /// And this far from the screen's top edge and the windows below.
    BAR_GAP: px = 4.0;
    /// The line between two groups in a pill.
    DIVIDER_HEIGHT: px = 16.0;
    DIVIDER_WIDTH: px = 1.0;
    /// Small marks, like a "wants attention" dot.
    DOT_SIZE: px = 4.0;
    /// The central pill's workspace numbers, each a round cell this big and
    /// this far apart, with extra room after the special workspaces. The
    /// search button beside them is a cell too.
    WORKSPACE_SIZE: px = 26.0;
    WORKSPACE_GAP: px = 2.0;
    WORKSPACE_GROUP_GAP: px = 4.0;
    /// Marks inside a round cell (a ring, a dot) keep this far inside its
    /// edge.
    RING_INSET: px = 2.0;
    /// While the bar steps aside for a fullscreen window: the strip along
    /// the top edge that brings it back when the pointer touches it.
    REVEAL_STRIP: px = 2.0;
    /// A label beside an icon sits this far right, to line up optically
    /// with the icon's edge.
    LABEL_NUDGE: px = 1.0;

    // Icons: in the bar's pills, in panels, and in menus.
    ICON_SIZE_SMALL: px = 14.0;
    ICON_SIZE: px = 16.0;
    ICON_SIZE_MENU: px = 18.0;
    /// Between an icon and the label or number beside it.
    ICON_GAP: px = 6.0;
    /// A line icon's stroke, on its 24-unit grid.
    ICON_STROKE: ratio = 1.8;
    /// Thin strokes, like a ring or a spinner's arc.
    STROKE_THIN: px = 1.5;
    /// A loading spinner's arc, in degrees of its ring.
    SPINNER_SWEEP: ratio = 86.0;

    // Panels: width, inner padding, the gap between sections, list rows
    // and meters.
    PANEL_WIDTH: px = 300.0;
    PANEL_PAD: px = 20.0;
    PANEL_PAD_BOTTOM: px = 16.0;
    SECTION_GAP: px = 18.0;
    ROW_HEIGHT: px = 40.0;
    /// Inner padding of a list row.
    ROW_INSET: px = 10.0;
    METER_HEIGHT: px = 8.0;
    METER_HEIGHT_SMALL: px = 4.0;
    /// Small labels in a pill shape, like a plan name.
    CHIP_HEIGHT: px = 22.0;
    CHIP_PAD: px = 10.0;
    /// A row of tabs (`crate::Segments`): its padding, also between two
    /// tabs, and a tab's height.
    SEGMENT_PAD: px = 4.0;
    SEGMENT_HEIGHT: px = 30.0;
    /// The text cursor in a field (`crate::TextField`).
    CARET_WIDTH: px = 2.0;
    /// The line under text an input method is still composing.
    COMPOSING_UNDERLINE: px = 1.0;
    /// An app's icon in a row (`crate::AppIcon`). Without an image it is
    /// the app's letter on a swatch, this round at this size, the letter
    /// this share of the tile.
    APP_ICON_SIZE: px = 30.0;
    LETTER_TILE_RADIUS: px = 9.0;
    LETTER_SCALE: ratio = 0.47;
    /// The round buttons inside a pill: this far from its ends and each
    /// other.
    PILL_BUTTON_SIZE: px = 28.0;
    PILL_BUTTON_INSET: px = 4.0;
    PILL_BUTTON_GAP: px = 2.0;
    /// A list row with a second line under its name (`crate::ListRow`).
    LIST_ROW_HEIGHT: px = 46.0;
    /// A tab against the right screen edge, halfway down (the power
    /// menu): its corners, the concave corners that join it to the edge,
    /// and the nub it grows out of.
    EDGE_TAB_RADIUS: px = 27.0;
    EDGE_TAB_FILLET: px = 22.0;
    EDGE_TAB_NUB: px = 40.0;
    /// The line that drains along the bottom of what waits for a second
    /// click (a power menu row, a screenshot's card), on the accent.
    DRAIN_HEIGHT: px = 3.0;
    DRAIN_OPACITY: ratio = 0.55;
    /// The padding inside a menu and a card that holds a list (a tray
    /// app's menu, the Devices and Tray cards).
    MENU_PAD: px = 6.0;

    // Quick settings' controls, which other features fill it with.
    /// Its header's logo, and how tall every page's top row is
    /// (`crate::PageHeader`).
    LOGO_SIZE: px = 36.0;
    /// Between the sections of its pages, its own and other features'.
    QUICK_SETTINGS_GAP: px = 12.0;
    /// How far quick settings' pages sit in from its edges: a page's top
    /// row leaves room for the pill's buttons by it
    /// (`page_header::header_inset`).
    QUICK_SETTINGS_PAD: px = 16.0;
    /// A toggle tile (`crate::Tile`), and the arrow at its end that opens
    /// its page.
    TILE_HEIGHT: px = 58.0;
    TILE_ARROW_WIDTH: px = 22.0;
    /// A thick slider (`crate::Slider`): its round icon button beside the
    /// track, the gap between, the fill (the ink at this opacity, less
    /// while muted) and the button after it that opens a page.
    SLIDER_HEIGHT: px = 34.0;
    SLIDER_GAP: px = 6.0;
    SLIDER_FILL_OPACITY: ratio = 0.22;
    SLIDER_MUTED_OPACITY: ratio = 0.3;
    SLIDER_BUTTON_WIDTH: px = 30.0;
    /// An app's slider shows the app's icon, its corners less round, and
    /// fainter while muted.
    SLIDER_APP_RADIUS: px = 11.0;
    SLIDER_APP_MUTED_OPACITY: ratio = 0.45;
    // Small controls on quick settings' pages: an on/off switch, the
    // buttons on a card that asks something, a device's battery (a rim with
    // a nub, filled to its level, with a bolt while charging), and the
    // lines of a graph over time.
    SWITCH_WIDTH: px = 40.0;
    SWITCH_HEIGHT: px = 24.0;
    SWITCH_KNOB: px = 18.0;
    CONFIRM_BUTTON_HEIGHT: px = 26.0;
    DEVICE_BATTERY_WIDTH: px = 22.0;
    DEVICE_BATTERY_HEIGHT: px = 11.0;
    DEVICE_BATTERY_RADIUS: px = 3.0;
    DEVICE_BATTERY_INSET: px = 2.0;
    DEVICE_BATTERY_NUB_WIDTH: px = 2.0;
    DEVICE_BATTERY_NUB_HEIGHT: px = 4.0;
    DEVICE_BATTERY_RIM: px = 1.25;
    DEVICE_BATTERY_RIM_OPACITY: ratio = 0.55;
    DEVICE_BOLT_SIZE: px = 9.0;
    GRAPH_LINE: px = 2.0;

    // The type scale: six steps, from labels up, then a panel's headline.
    /// Small caps labels and badges.
    FONT_SIZE_MICRO: px = 10.0;
    /// Secondary text: notification bodies, subtitles, times.
    FONT_SIZE_SMALL: px = 12.0;
    /// Body text: rows, names, the central pill's time.
    FONT_SIZE: px = 13.0;
    /// A page's title beside its back button, and the search field.
    FONT_SIZE_HEADING: px = 15.0;
    /// A name or number on its own.
    FONT_SIZE_TITLE: px = 18.0;
    /// Big words: a greeting, an empty list.
    FONT_SIZE_DISPLAY: px = 24.0;
    /// A panel's headline number.
    FONT_SIZE_HERO: px = 56.0;
    LINE_HEIGHT_HERO: px = 60.0;
    /// Line height for text that wraps, as a share of its size.
    LINE_HEIGHT_WRAPPED: ratio = 1.5;

    // Motion
    DURATION_FAST: ms = 120;
    /// A panel growing out of its pill, and back.
    DURATION_PANEL: ms = 250;
    /// Content fading in once a panel has grown.
    DURATION_FADE: ms = 140;
    /// Meters filling.
    DURATION_FILL: ms = 600;
    DURATION_COLOR: ms = 200;
    /// Something changing size as its content changes.
    DURATION_RESIZE: ms = 180;
    /// The bar stays this long after the pointer leaves it over a
    /// fullscreen window.
    DURATION_PEEK_LINGER: ms = 400;
    /// One turn of a loading spinner.
    DURATION_SPIN: ms = 1000;
    /// When a panel's content starts to show, as a share of its morph, and
    /// when its meters and rings start to fill.
    REVEAL_CONTENT: ratio = 0.5;
    REVEAL_FILL: ratio = 0.6;
    /// The scale something new grows in from.
    ENTER_SCALE: ratio = 0.6;
}

/// The room a glass's shadow needs around it.
pub const SHADOW_ROOM: Pixels = px(40.0);

/// How far the central pill's round cells (its search button, the
/// workspace numbers) sit from the pill's ends: as far as from its top,
/// `(BAR_HEIGHT - WORKSPACE_SIZE) / 2` (a test holds it so).
pub const WORKSPACE_INSET: Pixels = px(5.0);

/// A dashed ring's dashes and the gaps between them, as long as each other
/// (in units of the stroke's width).
pub const RING_DASH: f32 = 2.0;

/// The grid line icons are drawn on.
pub const ICON_GRID: f32 = 24.0;

/// The cubic Bézier every panel motion runs on (fast out, gentle stop), as
/// CSS writes it: x1, y1, x2, y2.
pub const EASING: [f32; 4] = [0.2, 0.8, 0.2, 1.0];

// Type: the families, and the weights of running text, of labels and
// names, of titles and picked options, and of a badge's count. A family
// draws with the face this system matches it to, as Qt's does ([`Family`]).
pub const FONT_SANS: Family = Family::named(nova_theme_info::FONT_SANS);
pub const FONT_MONO: Family = Family::named(nova_theme_info::FONT_MONO);
pub const FONT_DISPLAY: Family = Family::named(nova_theme_info::FONT_DISPLAY);
/// A line's height as a share of its text size: Inter's own spacing, which
/// is how Qt lays text out (GPUI's default is 1.618). `body_text` sets it,
/// so nothing else does.
pub const LINE_HEIGHT: f32 = 1.21;
pub const WEIGHT_BODY: FontWeight = FontWeight::NORMAL;
pub const WEIGHT_LABEL: FontWeight = FontWeight::MEDIUM;
pub const WEIGHT_TITLE: FontWeight = FontWeight::SEMIBOLD;
pub const WEIGHT_BADGE: FontWeight = FontWeight::BOLD;
