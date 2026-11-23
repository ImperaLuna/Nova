//! STAND-IN until IMP-411's `nova_look::DeviceBattery` lands: a device's
//! little battery, `DeviceBattery.qml` in today's shell.

use gpui::prelude::FluentBuilder as _;
use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div, px};
use nova_look::{Glyph, Icon, Theme};

const WIDTH: f32 = 22.0;
const HEIGHT: f32 = 11.0;
const RADIUS: f32 = 3.0;
const INSET: f32 = 2.0;
const NUB_WIDTH: f32 = 2.0;
const NUB_HEIGHT: f32 = 4.0;
const RIM: f32 = 1.25;
const RIM_OPACITY: f32 = 0.55;
const BOLT: f32 = 9.0;

#[derive(IntoElement)]
pub struct DeviceBattery {
    fill: f32,
    low: bool,
    charging: bool,
}

impl DeviceBattery {
    pub fn new(fill: f32, low: bool, charging: bool) -> Self {
        Self { fill, low, charging }
    }
}

impl RenderOnce for DeviceBattery {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let tone = if self.charging {
            theme.success
        } else if self.low {
            theme.error
        } else {
            theme.fg
        };
        let rim = if self.low && !self.charging { theme.error } else { theme.fg.alpha(RIM_OPACITY) };
        let inner = WIDTH - 2.0 * INSET;
        div()
            .flex_none()
            .flex()
            .items_center()
            .child(
                div()
                    .relative()
                    .w(px(WIDTH))
                    .h(px(HEIGHT))
                    .rounded(px(RADIUS))
                    .border(px(RIM))
                    .border_color(rim)
                    .child(
                        div()
                            .absolute()
                            .left(px(INSET - RIM))
                            .top(px(INSET - RIM))
                            .h(px(HEIGHT - 2.0 * INSET))
                            .w(px(inner * self.fill.clamp(0.0, 1.0)))
                            .rounded(px(RADIUS - INSET / 2.0))
                            .bg(tone),
                    )
                    .when(self.charging, |body| {
                        body.child(
                            div().absolute().inset_0().flex().items_center().justify_center().child(
                                Icon::new(Glyph::BOLT).filled(Glyph::BOLT).size(px(BOLT)).color(theme.on_accent),
                            ),
                        )
                    }),
            )
            .child(div().w(px(NUB_WIDTH)).h(px(NUB_HEIGHT)).rounded(px(NUB_WIDTH / 2.0)).bg(rim))
    }
}
