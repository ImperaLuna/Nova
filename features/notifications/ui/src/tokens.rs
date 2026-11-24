use gpui::{Pixels, px};
use std::time::Duration;

pub const WIDTH: Pixels = px(380.0);
pub const SHOWN: usize = 5;
pub const STAGGER: Duration = Duration::from_millis(60);
pub const TIMEOUT: Duration = Duration::from_millis(6000);
pub const RADIUS: Pixels = px(20.0);
pub const PAD_BOTTOM: Pixels = px(14.0);
pub const AVATAR: Pixels = px(40.0);
pub const APP_LINE: Pixels = px(18.0);
pub const LINE_GAP: Pixels = px(2.0);
pub const BODY_LINES: usize = 3;
pub const ACTIONS_GAP: Pixels = px(8.0);
pub const ACTION_HEIGHT: Pixels = px(28.0);
pub const PROGRESS_HEIGHT: Pixels = px(6.0);
pub const TIMER_HEIGHT: Pixels = px(2.0);
pub const COUNTDOWN_TICK: Duration = Duration::from_millis(83);
pub const TIMER_OPACITY: f32 = 0.35;
pub const GROW_WAIT: Duration = Duration::from_millis(100);
pub const CRITICAL_RIM: Pixels = px(1.5);
pub const DEVICE_RADIUS: f32 = 0.3;
pub const DEVICE_GLYPH: f32 = 0.55;
pub const CHIP_HEIGHT: Pixels = px(16.0);
pub const CHIP_PAD: Pixels = px(6.0);
pub const CLOSE_SIZE: Pixels = px(24.0);
pub const CLOSE_INSET: Pixels = px(9.0);
pub const FOLD: f32 = 0.08;
pub const FOLD_HEIGHT: f32 = 0.15;
pub const RISE: Pixels = px(38.0);
pub const AWAY: Pixels = px(60.0);

pub const BUTTON: Pixels = px(26.0);
pub const ROW_ICON: Pixels = px(28.0);
pub const ROW_LINES: usize = 2;
pub const LIST_GAP: Pixels = px(6.0);
pub const PEEK: Pixels = px(6.0);
pub const PEEK_HEIGHT: Pixels = px(14.0);
pub const PEEK_INSET: Pixels = px(10.0);
pub const MENU_WIDTH: Pixels = px(170.0);
pub const EMPTY_ICON: Pixels = px(28.0);

pub const MENU_PAD: Pixels = px(4.0);
pub const MENU_ITEM_HEIGHT: Pixels = px(30.0);

pub const LETTER_TILE_RADIUS: Pixels = px(9.0);
pub const LETTER_TILE_FOR: Pixels = px(30.0);
pub const LETTER_SCALE: f32 = 0.47;
