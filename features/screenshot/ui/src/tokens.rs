use gpui::{Hsla, Pixels, black, px};
use std::time::Duration;

pub const VEIL_OPACITY: f32 = 0.45;
pub const OUTLINE: Pixels = px(1.5);

pub fn veil() -> Hsla {
    black().opacity(VEIL_OPACITY)
}
pub const MIN_DRAG: Pixels = px(4.0);
pub const BAR_HEIGHT: Pixels = px(44.0);
pub const BUTTON_HEIGHT: Pixels = px(32.0);
pub const BUTTON_PAD: Pixels = px(12.0);
pub const SWITCH_WIDTH: Pixels = px(76.0);
pub const SWITCH_PAD: Pixels = px(2.0);
pub const BADGE_HEIGHT: Pixels = px(26.0);
pub const CARD_WIDTH: Pixels = px(360.0);
pub const THUMB_HEIGHT: Pixels = px(184.0);
pub const CARD_BUTTON_HEIGHT: Pixels = px(36.0);
pub const CARD_GAP: Pixels = px(12.0);
pub const CARD_TIME: Duration = Duration::from_millis(6000);
pub const DRAIN_HEIGHT: Pixels = px(3.0);
pub const DRAIN_OPACITY: f32 = 0.55;
pub const COUNTDOWN_TICK: Duration = Duration::from_millis(83);
pub const FAILED_CARD_TIME: Duration = Duration::from_millis(10000);
pub const FAILED_CARD_LINES: usize = 3;
pub const RECORD_COUNTDOWN: u32 = 3;
pub const COUNTDOWN_STEP: Duration = Duration::from_millis(1000);
pub const PICKER_GONE_DELAY: Duration = Duration::from_millis(150);
pub const FONT_SIZE_COUNTDOWN: Pixels = px(96.0);
pub const RECORD_DOT: Pixels = px(10.0);
pub const MARK_LENGTH: Pixels = px(18.0);
pub const MARK_WIDTH: Pixels = px(3.0);
pub const MARK_GAP: Pixels = px(4.0);
