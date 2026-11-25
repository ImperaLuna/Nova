use crate::tokens::{
    BAR_MARGIN, FONT_SIZE_HEADING, GAP, LABEL_NUDGE, LOGO_SIZE, PILL_BUTTON_GAP, PILL_BUTTON_INSET, PILL_BUTTON_SIZE,
    QUICK_SETTINGS_PAD, WEIGHT_TITLE,
};
use crate::{Glyph, Icon, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use std::rc::Rc;

type Clicked = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

pub const PILL_BUTTONS: &str = "right-pill.buttons";

pub fn header_inset(buttons: usize) -> Pixels {
    if buttons == 0 {
        return px(0.0);
    }
    let count = buttons as f32;
    let row = PILL_BUTTON_SIZE * count + PILL_BUTTON_GAP * (count - 1.0);
    BAR_MARGIN + PILL_BUTTON_INSET * 2.0 + row - QUICK_SETTINGS_PAD + GAP
}

#[derive(IntoElement)]
pub struct PageHeader {
    title: SharedString,
    on_back: Clicked,
    end: Option<AnyElement>,
}

impl PageHeader {
    pub fn new(title: impl Into<SharedString>, back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        Self { title: title.into(), on_back: Rc::new(back), end: None }
    }

    pub fn end(mut self, end: impl IntoElement) -> Self {
        self.end = Some(end.into_any_element());
        self
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
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(FONT_SIZE_HEADING)
                    .font_weight(WEIGHT_TITLE)
                    .child(self.title),
            )
            .when_some(self.end, |row, end| row.child(div().flex_none().child(end)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_top_row_leaves_room_for_the_pill_s_buttons() {
        assert_eq!(header_inset(0), px(0.0));
        assert_eq!(header_inset(1), px(8.0 + 8.0 + 28.0 - 16.0 + 8.0));
        assert_eq!(header_inset(2), px(8.0 + 8.0 + 58.0 - 16.0 + 8.0));
    }
}
