//! A quick settings page's top row: a round back button and the page's
//! title. `ui/quick_settings/PageHeader.qml` in today's shell.

use crate::tokens::{FONT_SIZE_HEADING, GAP, LABEL_NUDGE, LOGO_SIZE, PILL_BUTTON_SIZE, WEIGHT_TITLE};
use crate::{Glyph, Icon, Theme};
use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};
use std::rc::Rc;

type Clicked = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A page's top row (see the module).
#[derive(IntoElement)]
pub struct PageHeader {
    title: SharedString,
    on_back: Clicked,
}

impl PageHeader {
    pub fn new(title: impl Into<SharedString>, back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        Self { title: title.into(), on_back: Rc::new(back) }
    }
}

impl RenderOnce for PageHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let hover = theme.hover();
        let back = self.on_back;
        div()
            .flex_none()
            .h(LOGO_SIZE)
            .flex()
            .items_center()
            .gap(GAP)
            .child(
                div()
                    .id("back")
                    .size(PILL_BUTTON_SIZE + LABEL_NUDGE * 2.0)
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(theme.surface_raised())
                    .hover(move |style| style.bg(hover))
                    .on_click(move |event, window, cx| back(event, window, cx))
                    .child(Icon::new(Glyph::LEFT)),
            )
            .child(div().text_size(FONT_SIZE_HEADING).font_weight(WEIGHT_TITLE).child(self.title))
    }
}
