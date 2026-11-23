//! The notifications' own sizes and timings.

use gpui::{Pixels, px};
use std::time::Duration;

// Pop-ups: this wide under the right pill, at most SHOWN at once, each
// staying TIMEOUT unless its app asks otherwise; several coming at once
// open into their slots STAGGER apart.
pub const WIDTH: Pixels = px(380.0);
pub const SHOWN: usize = 5;
pub const STAGGER: Duration = Duration::from_millis(60);
pub const TIMEOUT: Duration = Duration::from_millis(6000);
pub const RADIUS: Pixels = px(20.0);
pub const PAD_BOTTOM: Pixels = px(14.0);
/// Who it's from: a picture, the app's icon or a device.
pub const AVATAR: Pixels = px(40.0);
pub const APP_LINE: Pixels = px(18.0);
pub const LINE_GAP: Pixels = px(2.0);
pub const BODY_LINES: usize = 3;
pub const ACTIONS_GAP: Pixels = px(8.0);
pub const ACTION_HEIGHT: Pixels = px(28.0);
pub const PROGRESS_HEIGHT: Pixels = px(6.0);
/// The line along a card's bottom that drains over its time, moving every
/// COUNTDOWN_TICK (12 a second) while nothing else does: smooth to the eye,
/// a few frames, not one each vblank.
pub const TIMER_HEIGHT: Pixels = px(2.0);
pub const COUNTDOWN_TICK: Duration = Duration::from_millis(83);
pub const TIMER_OPACITY: f32 = 0.35;
/// The longest new pop-ups wait for their window to grow before they open
/// in the one it has: a compositor answers within a frame or two.
pub const GROW_WAIT: Duration = Duration::from_millis(100);
/// A critical one's rim, in the error color.
pub const CRITICAL_RIM: Pixels = px(1.5);
/// A low battery alert's icon: the device's glyph on a rounded tile, its
/// radius and the glyph a share of the icon's size.
pub const DEVICE_RADIUS: f32 = 0.3;
pub const DEVICE_GLYPH: f32 = 0.55;
/// Counts in the list: its heading's, and "+2" on a stack of an app's.
pub const CHIP_HEIGHT: Pixels = px(16.0);
pub const CHIP_PAD: Pixels = px(6.0);
/// The ✕ and ⋯ on a card's corner, shown on hover.
pub const CLOSE_SIZE: Pixels = px(24.0);
pub const CLOSE_INSET: Pixels = px(9.0);
/// A pop-up grows out of the quick settings button: from this share of its
/// width and height, this far above its place. One that's clicked or
/// dismissed slides this far right as it fades.
pub const FOLD: f32 = 0.08;
pub const FOLD_HEIGHT: f32 = 0.15;
pub const RISE: Pixels = px(38.0);
pub const AWAY: Pixels = px(60.0);

// The list in quick settings: the heading's round buttons, its rows (an
// icon, two lines of body), the gap between them, a stack's edges under
// its top one, the ⋯ menu and the empty list's bell.
pub const BUTTON: Pixels = px(26.0);
pub const ROW_ICON: Pixels = px(28.0);
pub const ROW_LINES: usize = 2;
pub const LIST_GAP: Pixels = px(6.0);
pub const PEEK: Pixels = px(6.0);
pub const PEEK_HEIGHT: Pixels = px(14.0);
pub const PEEK_INSET: Pixels = px(10.0);
pub const MENU_WIDTH: Pixels = px(170.0);
pub const EMPTY_ICON: Pixels = px(28.0);

/// The ⋯ menu's inner padding and its items' height.
pub const MENU_PAD: Pixels = px(4.0);
pub const MENU_ITEM_HEIGHT: Pixels = px(30.0);

/// An app with no icon: its letter on a swatch, this round and this big for
/// the launcher's row icon, scaled for other sizes.
pub const LETTER_TILE_RADIUS: Pixels = px(9.0);
pub const LETTER_TILE_FOR: Pixels = px(30.0);
pub const LETTER_SCALE: f32 = 0.47;
