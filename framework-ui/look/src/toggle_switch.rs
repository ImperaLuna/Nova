//! An on/off switch: the accent while on, a quiet track while off, its knob
//! across at the side it is on. It only shows; the row it sits in takes the
//! click.
//!
//! The knob slides when it flips: its owner keeps where it is
//! ([`crate::motion::Eased`] over `DURATION_COLOR`) and passes it with
//! [`ToggleSwitch::along`], asking for frames while it moves. Without it the
//! knob sits at its side.

use crate::tokens::{SWITCH_HEIGHT, SWITCH_KNOB, SWITCH_WIDTH};
use crate::{Theme, mix};
use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};

/// An on/off switch (see the module).
#[derive(Debug, Clone, Copy, IntoElement)]
pub struct ToggleSwitch {
    on: bool,
    along: Option<f32>,
}

impl ToggleSwitch {
    pub fn new(on: bool) -> Self {
        Self { on, along: None }
    }

    /// How far across the knob is, 0 (off's side) to 1 (on's), while it
    /// slides; the track's fill follows it.
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
