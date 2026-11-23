//! A device's little battery: a rim with a nub, filled to its level, in the
//! error color while low and the success color with a bolt while charging.
//! The Bluetooth page's rows, the Devices card and a low battery alert draw
//! it. `ui/quick_settings/DeviceBattery.qml` in today's shell.
//!
//! A new level eases in when its owner passes an eased fill: the battery
//! itself keeps nothing.

use crate::tokens::{
    DEVICE_BATTERY_HEIGHT, DEVICE_BATTERY_INSET, DEVICE_BATTERY_NUB_HEIGHT, DEVICE_BATTERY_NUB_WIDTH,
    DEVICE_BATTERY_RADIUS, DEVICE_BATTERY_RIM, DEVICE_BATTERY_RIM_OPACITY, DEVICE_BATTERY_WIDTH, DEVICE_BOLT_SIZE,
};
use crate::{Glyph, Icon, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};

/// A device's battery (see the module).
#[derive(Debug, Clone, Copy, IntoElement)]
pub struct DeviceBattery {
    fill: f32,
    low: bool,
    charging: bool,
}

/// At or under this percent a battery reads as low (`devices.js`).
pub const LOW_PERCENT: u8 = 15;

impl DeviceBattery {
    /// A battery at `percent`, low at or under [`LOW_PERCENT`].
    pub fn percent(percent: u8) -> Self {
        Self::new(f32::from(percent.min(100)) / 100.0).low(percent <= LOW_PERCENT)
    }

    /// Filled `fill` of the way (0 to 1).
    pub fn new(fill: f32) -> Self {
        Self { fill: fill.clamp(0.0, 1.0), low: false, charging: false }
    }

    /// In the error color, rim and all.
    pub fn low(mut self, low: bool) -> Self {
        self.low = low;
        self
    }

    /// In the success color, with a bolt.
    pub fn charging(mut self, charging: bool) -> Self {
        self.charging = charging;
        self
    }
}

impl RenderOnce for DeviceBattery {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let tone = match (self.charging, self.low) {
            (true, _) => theme.success,
            (false, true) => theme.error,
            (false, false) => theme.fg,
        };
        let rim = if self.low && !self.charging { theme.error } else { theme.fg.alpha(DEVICE_BATTERY_RIM_OPACITY) };
        // Placed from inside the rim, where absolute children start; the
        // inset counts from the outer edge.
        let inset = DEVICE_BATTERY_INSET - DEVICE_BATTERY_RIM;
        let level = div()
            .absolute()
            .top(inset)
            .left(inset)
            .h(DEVICE_BATTERY_HEIGHT - DEVICE_BATTERY_INSET * 2.0)
            .w((DEVICE_BATTERY_WIDTH - DEVICE_BATTERY_INSET * 2.0) * self.fill)
            .rounded(DEVICE_BATTERY_RADIUS - DEVICE_BATTERY_INSET / 2.0)
            .bg(tone);
        let body = div()
            .relative()
            .flex_none()
            .w(DEVICE_BATTERY_WIDTH)
            .h(DEVICE_BATTERY_HEIGHT)
            .rounded(DEVICE_BATTERY_RADIUS)
            .border(DEVICE_BATTERY_RIM)
            .border_color(rim)
            .flex()
            .items_center()
            .justify_center()
            .child(level)
            .when(self.charging, |body| {
                body.child(Icon::new(Glyph::BOLT).filled(Glyph::BOLT).size(DEVICE_BOLT_SIZE).color(theme.on_accent))
            });
        let nub = div().flex_none().w(DEVICE_BATTERY_NUB_WIDTH).h(DEVICE_BATTERY_NUB_HEIGHT).rounded_full().bg(rim);
        div().flex_none().flex().items_center().child(body).child(nub)
    }
}
