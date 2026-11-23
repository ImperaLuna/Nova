//! A card's small uppercase heading, like "Resources" or "Nearby".
//! `ui/quick_settings/CardTitle.qml` in today's shell, without its wider
//! letter spacing: GPUI's text has none.

use crate::Theme;
use crate::tokens::FONT_SIZE_SMALL;
use gpui::{App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div};

/// A card's heading (see the module).
#[derive(IntoElement)]
pub struct CardTitle {
    text: SharedString,
}

impl CardTitle {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self { text: text.into() }
    }
}

impl RenderOnce for CardTitle {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .flex_none()
            .text_size(FONT_SIZE_SMALL)
            .text_color(Theme::of(cx).fg_soft())
            .child(self.text.to_uppercase())
    }
}
