use crate::tokens::{SWITCH_HEIGHT, SWITCH_KNOB, SWITCH_WIDTH};
use crate::{Theme, mix};
use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};

#[derive(Debug, Clone, Copy, IntoElement)]
pub struct ToggleSwitch {
    on: bool,
    along: Option<f32>,
}

impl ToggleSwitch {
    pub fn new(on: bool) -> Self {
        Self { on, along: None }
    }

    pub fn along(mut self, along: f32) -> Self {
        self.along = Some(along.clamp(0.0, 1.0));
        self
    }
}

impl RenderOnce for ToggleSwitch {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let along = self.along.unwrap_or(if self.on { 1.0 } else { 0.0 });
        let inset = (SWITCH_HEIGHT - SWITCH_KNOB) / 2.0;
        let knob = div()
            .absolute()
            .top(inset)
            .left(inset + (SWITCH_WIDTH - SWITCH_KNOB - inset * 2.0) * along)
            .size(SWITCH_KNOB)
            .rounded_full()
            .bg(if self.on { theme.on_accent } else { theme.fg_soft() });
        div()
            .relative()
            .flex_none()
            .w(SWITCH_WIDTH)
            .h(SWITCH_HEIGHT)
            .rounded_full()
            .bg(mix(theme.track(), theme.primary, along))
            .child(knob)
    }
}
