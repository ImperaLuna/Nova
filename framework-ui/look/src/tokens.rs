use crate::Family;
use gpui::{FontWeight, Pixels, px};
use std::time::Duration;

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
    FG_DIM_OPACITY: ratio = 0.66;
    FG_SOFT_OPACITY: ratio = 0.72;
    DIM_OPACITY: ratio = 0.5;

    SURFACE_OPACITY: ratio = 0.75;
    RAISED_OPACITY: ratio = 0.09;
    OUTLINE_OPACITY: ratio = 0.08;
    HOVER_OPACITY: ratio = 0.06;
    SELECTED_OPACITY: ratio = 0.18;
    SELECTED_QUIET_OPACITY: ratio = 0.14;
    ACCENT_FILL_OPACITY: ratio = 0.92;
    TRACK_OPACITY: ratio = 0.14;
    METER_OPACITY: ratio = 0.85;

    GLASS_OPACITY: ratio = 0.55;
    GLASS_OUTLINE_OPACITY: ratio = 0.45;
    GLASS_INNER_LINE_OPACITY: ratio = 0.06;
    GLASS_HIGHLIGHT_OPACITY: ratio = 0.22;
    GLASS_HIGHLIGHT_INSET: ratio = 0.7;
    GLASS_HIGHLIGHT_FADE: ratio = 0.15;
    GLASS_EDGE_WIDTH: px = 1.0;
    GLASS_RIM_OPACITY: ratio = 0.10;
    SHADOW_OPACITY: ratio = 0.35;
    SHADOW_BLUR: px = 30.0;
    SHADOW_OFFSET: px = 10.0;

    RADIUS_PANEL: px = 24.0;
    RADIUS_CARD: px = 16.0;
    RADIUS_CONTROL: px = 12.0;
    RADIUS_ROW: px = 14.0;
    RADIUS_SEGMENT: px = 10.0;

    GAP: px = 8.0;
    PAD: px = 12.0;
    BAR_HEIGHT: px = 36.0;
    BAR_MARGIN: px = 8.0;
    BAR_GAP: px = 4.0;
    DIVIDER_HEIGHT: px = 16.0;
    DIVIDER_WIDTH: px = 1.0;
    DOT_SIZE: px = 4.0;
    WORKSPACE_SIZE: px = 26.0;
    WORKSPACE_GAP: px = 2.0;
    WORKSPACE_GROUP_GAP: px = 4.0;
    RING_INSET: px = 2.0;
    REVEAL_STRIP: px = 2.0;
    LABEL_NUDGE: px = 1.0;

    ICON_SIZE_SMALL: px = 14.0;
    ICON_SIZE: px = 16.0;
    ICON_SIZE_MENU: px = 18.0;
    ICON_GAP: px = 6.0;
    ICON_STROKE: ratio = 1.8;
    STROKE_THIN: px = 1.5;
    SPINNER_SWEEP: ratio = 86.0;

    PANEL_WIDTH: px = 300.0;
    PANEL_PAD: px = 20.0;
    PANEL_PAD_BOTTOM: px = 16.0;
    SECTION_GAP: px = 18.0;
    ROW_HEIGHT: px = 40.0;
    ROW_INSET: px = 10.0;
    METER_HEIGHT: px = 8.0;
    METER_HEIGHT_SMALL: px = 4.0;
    CHIP_HEIGHT: px = 22.0;
    CHIP_PAD: px = 10.0;
    SEGMENT_PAD: px = 4.0;
    SEGMENT_HEIGHT: px = 30.0;
    CARET_WIDTH: px = 2.0;
    COMPOSING_UNDERLINE: px = 1.0;
    APP_ICON_SIZE: px = 30.0;
    LETTER_TILE_RADIUS: px = 9.0;
    LETTER_SCALE: ratio = 0.47;
    PILL_BUTTON_SIZE: px = 28.0;
    PILL_BUTTON_INSET: px = 4.0;
    PILL_BUTTON_GAP: px = 2.0;
    LIST_ROW_HEIGHT: px = 46.0;
    EDGE_TAB_RADIUS: px = 27.0;
    EDGE_TAB_FILLET: px = 22.0;
    EDGE_TAB_NUB: px = 40.0;
    DRAIN_HEIGHT: px = 3.0;
    DRAIN_OPACITY: ratio = 0.55;
    MENU_PAD: px = 6.0;

    LOGO_SIZE: px = 36.0;
    QUICK_SETTINGS_GAP: px = 12.0;
    QUICK_SETTINGS_PAD: px = 16.0;
    TILE_HEIGHT: px = 58.0;
    TILE_ARROW_WIDTH: px = 22.0;
    SLIDER_HEIGHT: px = 34.0;
    SLIDER_GAP: px = 6.0;
    SLIDER_FILL_OPACITY: ratio = 0.22;
    SLIDER_MUTED_OPACITY: ratio = 0.3;
    SLIDER_BUTTON_WIDTH: px = 30.0;
    SLIDER_APP_RADIUS: px = 11.0;
    SLIDER_APP_MUTED_OPACITY: ratio = 0.45;
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

    FONT_SIZE_MICRO: px = 10.0;
    FONT_SIZE_SMALL: px = 12.0;
    FONT_SIZE: px = 13.0;
    FONT_SIZE_HEADING: px = 15.0;
    FONT_SIZE_TITLE: px = 18.0;
    FONT_SIZE_DISPLAY: px = 24.0;
    FONT_SIZE_HERO: px = 56.0;
    LINE_HEIGHT_HERO: px = 60.0;
    LINE_HEIGHT_WRAPPED: ratio = 1.5;

    DURATION_FAST: ms = 120;
    DURATION_PANEL: ms = 250;
    DURATION_FADE: ms = 140;
    DURATION_FILL: ms = 600;
    DURATION_COLOR: ms = 200;
    DURATION_RESIZE: ms = 180;
    DURATION_PEEK_LINGER: ms = 400;
    DURATION_SPIN: ms = 1000;
    REVEAL_CONTENT: ratio = 0.5;
    REVEAL_FILL: ratio = 0.6;
    ENTER_SCALE: ratio = 0.6;
}

pub const SHADOW_ROOM: Pixels = px(40.0);

pub const WORKSPACE_INSET: Pixels = px(5.0);

pub const RING_DASH: f32 = 2.0;

pub const ICON_GRID: f32 = 24.0;

pub const EASING: [f32; 4] = [0.2, 0.8, 0.2, 1.0];

pub const FONT_SANS: Family = Family::named(nova_theme_info::FONT_SANS);
pub const FONT_MONO: Family = Family::named(nova_theme_info::FONT_MONO);
pub const FONT_DISPLAY: Family = Family::named(nova_theme_info::FONT_DISPLAY);
pub const LINE_HEIGHT: f32 = 1.21;
pub const WEIGHT_BODY: FontWeight = FontWeight::NORMAL;
pub const WEIGHT_LABEL: FontWeight = FontWeight::MEDIUM;
pub const WEIGHT_TITLE: FontWeight = FontWeight::SEMIBOLD;
pub const WEIGHT_BADGE: FontWeight = FontWeight::BOLD;
