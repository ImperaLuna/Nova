//! A small button on a card that asks something: quiet, or filled with the
//! accent for the answer that goes ahead, dimmed while it can't be
//! pressed. A pairing question's answers, a failure's Try again, a Wi-Fi
//! password's Connect. `ui/quick_settings/PromptButton.qml` in today's
//! shell.

use crate::tokens::{CONFIRM_BUTTON_HEIGHT, DIM_OPACITY, FONT_SIZE_SMALL, PAD, RADIUS_SEGMENT, WEIGHT_BODY, WEIGHT_TITLE};
use crate::Theme;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};
use std::rc::Rc;

type Clicked = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A card's button (see the module).
#[derive(IntoElement)]
pub struct PromptButton {
    id: ElementId,
    label: SharedString,
    accent: bool,
    enabled: bool,
    on_click: Option<Clicked>,
}

impl PromptButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self { id: id.into(), label: label.into(), accent: false, enabled: true, on_click: None }
    }

    /// Filled with the accent: the answer that goes ahead.
    pub fn accent(mut self, accent: bool) -> Self {
        self.accent = accent;
        self
    }

    /// `false` dims it and takes no clicks.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn on_click(mut self, clicked: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(clicked));
        self
    }
}

impl RenderOnce for PromptButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let hover = theme.hover();
        let (accent, enabled) = (self.accent, self.enabled);
        div()
            .id(self.id)
            .flex_none()
            .h(CONFIRM_BUTTON_HEIGHT)
            .px(PAD)
            .rounded(RADIUS_SEGMENT)
            .flex()
            .items_center()
            .justify_center()
            .text_size(FONT_SIZE_SMALL)
            .font_weight(if accent { WEIGHT_TITLE } else { WEIGHT_BODY })
            .text_color(if accent { theme.on_accent } else { theme.fg })
            .bg(if accent { theme.primary } else { theme.track() })
            .when(!enabled, |button| button.opacity(DIM_OPACITY))
            .when(enabled && !accent, |button| button.hover(move |style| style.bg(hover)))
            .when_some(self.on_click.filter(|_| enabled), |button, clicked| {
                button.on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    clicked(event, window, cx);
                })
            })
            .child(self.label)
    }
}
