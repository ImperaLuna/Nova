//! A round button inside a pill: the right pill's quick settings and power
//! buttons, and the same buttons where quick settings draws them over the
//! pill. `ui/bar/PillButton.qml` in today's shell.

use crate::tokens::{ICON_SIZE, PILL_BUTTON_SIZE};
use crate::{Glyph, Icon, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    StatefulInteractiveElement, Styled, Window, div,
};
use std::rc::Rc;

type Clicked = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A round button with a line icon: washed while the pointer is on it, a
/// quiet fill while `picked` (its panel is open). What sits on its corner,
/// like a badge, comes in with [`PillButton::child`].
#[derive(IntoElement)]
pub struct PillButton {
    id: ElementId,
    glyph: Glyph,
    picked: bool,
    on_click: Option<Clicked>,
    extra: Option<AnyElement>,
}

impl PillButton {
    pub fn new(id: impl Into<ElementId>, glyph: Glyph) -> Self {
        Self { id: id.into(), glyph, picked: false, on_click: None, extra: None }
    }

    pub fn picked(mut self, picked: bool) -> Self {
        self.picked = picked;
        self
    }

    pub fn on_click(mut self, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(on_click));
        self
    }

    /// Drawn over the button, placed by itself (`absolute`).
    pub fn child(mut self, extra: impl IntoElement) -> Self {
        self.extra = Some(extra.into_any_element());
        self
    }
}

impl RenderOnce for PillButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let hover = theme.hover();
        div()
            .id(self.id)
            .relative()
            .flex_none()
            .size(PILL_BUTTON_SIZE)
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .map(|button| match self.picked {
                true => button.bg(theme.selected_quiet()),
                false => button.hover(move |style| style.bg(hover)),
            })
            .when_some(self.on_click, |button, clicked| {
                button.on_click(move |event, window, cx| clicked(event, window, cx))
            })
            .child(Icon::new(self.glyph).size(ICON_SIZE))
            .children(self.extra)
    }
}
