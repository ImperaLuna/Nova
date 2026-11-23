//! The notifications' own sizes and timings. Until the Quickshell shell is
//! removed the same values live in `ui/theme/Tokens.qml`, under the names
//! the test below lists: change a value in both.

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

/// The settings page's on/off switches (their rows are `LIST_ROW_HEIGHT`).
pub const SWITCH_WIDTH: Pixels = px(40.0);
pub const SWITCH_HEIGHT: Pixels = px(24.0);
pub const SWITCH_KNOB: Pixels = px(18.0);
/// The ⋯ menu's inner padding and its items' height.
pub const MENU_PAD: Pixels = px(4.0);
pub const MENU_ITEM_HEIGHT: Pixels = px(30.0);

/// A low battery alert's battery, as the Devices card draws it.
pub const BATTERY_WIDTH: Pixels = px(22.0);
pub const BATTERY_HEIGHT: Pixels = px(11.0);
pub const BATTERY_RADIUS: Pixels = px(3.0);
pub const BATTERY_INSET: Pixels = px(2.0);
pub const BATTERY_NUB_WIDTH: Pixels = px(2.0);
pub const BATTERY_NUB_HEIGHT: Pixels = px(4.0);
pub const BATTERY_RIM: Pixels = px(1.25);

/// An app with no icon: its letter on a swatch, this round and this big for
/// the launcher's row icon, scaled for other sizes.
pub const LETTER_TILE_RADIUS: Pixels = px(9.0);
pub const LETTER_TILE_FOR: Pixels = px(30.0);
pub const LETTER_SCALE: f32 = 0.47;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const TOKENS_QML: &str = include_str!("../../../../ui/theme/Tokens.qml");

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

    fn number(value: &str) -> Option<f64> {
        let opacity = value.strip_suffix(')').and_then(|call| call.rsplit(',').next());
        opacity.unwrap_or(value).trim().parse().ok()
    }

    #[test]
    fn every_token_has_tokens_qml_s_value() {
        let pixels = |value: Pixels| f64::from(f32::from(value));
        let ms = |value: Duration| value.as_millis() as f64;
        let ours: &[(&str, f64)] = &[
            ("notificationWidth", pixels(WIDTH)),
            ("notificationsShown", SHOWN as f64),
            ("notificationStagger", ms(STAGGER)),
            ("durationNotification", ms(TIMEOUT)),
            ("radiusNotification", pixels(RADIUS)),
            ("notificationPadBottom", pixels(PAD_BOTTOM)),
            ("notificationAvatar", pixels(AVATAR)),
            ("notificationAppLine", pixels(APP_LINE)),
            ("notificationLineGap", pixels(LINE_GAP)),
            ("notificationBodyLines", BODY_LINES as f64),
            ("notificationActionsGap", pixels(ACTIONS_GAP)),
            ("notificationActionHeight", pixels(ACTION_HEIGHT)),
            ("notificationProgressHeight", pixels(PROGRESS_HEIGHT)),
            ("notificationTimerHeight", pixels(TIMER_HEIGHT)),
            ("countdownTick", ms(COUNTDOWN_TICK)),
            ("notificationTimer", f64::from(TIMER_OPACITY)),
            ("notificationCriticalRim", pixels(CRITICAL_RIM)),
            ("notificationDeviceRadius", f64::from(DEVICE_RADIUS)),
            ("notificationDeviceGlyph", f64::from(DEVICE_GLYPH)),
            ("notificationChipHeight", pixels(CHIP_HEIGHT)),
            ("notificationChipPad", pixels(CHIP_PAD)),
            ("notificationCloseSize", pixels(CLOSE_SIZE)),
            ("notificationCloseInset", pixels(CLOSE_INSET)),
            ("notificationFold", f64::from(FOLD)),
            ("notificationFoldHeight", f64::from(FOLD_HEIGHT)),
            ("notificationRise", pixels(RISE)),
            ("notificationAway", pixels(AWAY)),
            ("notificationButton", pixels(BUTTON)),
            ("notificationRowIcon", pixels(ROW_ICON)),
            ("notificationRowLines", ROW_LINES as f64),
            ("notificationListGap", pixels(LIST_GAP)),
            ("notificationPeek", pixels(PEEK)),
            ("notificationPeekHeight", pixels(PEEK_HEIGHT)),
            ("notificationPeekInset", pixels(PEEK_INSET)),
            ("notificationMenuWidth", pixels(MENU_WIDTH)),
            ("notificationEmptyIcon", pixels(EMPTY_ICON)),
            ("switchWidth", pixels(SWITCH_WIDTH)),
            ("switchHeight", pixels(SWITCH_HEIGHT)),
            ("switchKnob", pixels(SWITCH_KNOB)),
            ("segmentPad", pixels(MENU_PAD)),
            ("segmentHeight", pixels(MENU_ITEM_HEIGHT)),
            ("deviceBatteryWidth", pixels(BATTERY_WIDTH)),
            ("deviceBatteryHeight", pixels(BATTERY_HEIGHT)),
            ("deviceBatteryRadius", pixels(BATTERY_RADIUS)),
            ("deviceBatteryInset", pixels(BATTERY_INSET)),
            ("deviceBatteryNubWidth", pixels(BATTERY_NUB_WIDTH)),
            ("deviceBatteryNubHeight", pixels(BATTERY_NUB_HEIGHT)),
            ("deviceBatteryRim", pixels(BATTERY_RIM)),
            ("letterTileRadius", pixels(LETTER_TILE_RADIUS)),
            ("launcherRowIcon", pixels(LETTER_TILE_FOR)),
            ("letterScale", f64::from(LETTER_SCALE)),
        ];
        let qml = qml_properties();
        for (name, value) in ours {
            let text = qml.get(name).unwrap_or_else(|| panic!("Tokens.qml has no {name}"));
            let theirs = number(text).unwrap_or_else(|| panic!("Tokens.qml's {name} isn't a number: {text}"));
            assert!((theirs - value).abs() < 1e-6, "{name} is {value} here and {theirs} in Tokens.qml");
        }
    }
}
