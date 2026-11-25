use crate::tokens::{DURATION_FAST, ICON_SIZE, PILL_BUTTON_SIZE};
use crate::{Glyph, Hovered, Icon, Theme, fade_color, keyed};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, ElementId, Hsla, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    StatefulInteractiveElement, Styled, Window, div, transparent_black,
};
use std::rc::Rc;

type Clicked = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct PillButton {
    id: ElementId,
    glyph: Glyph,
    ink: Option<Hsla>,
    picked: bool,
    on_click: Option<Clicked>,
    extra: Option<AnyElement>,
}

impl PillButton {
    pub fn new(id: impl Into<ElementId>, glyph: Glyph) -> Self {
        Self { id: id.into(), glyph, ink: None, picked: false, on_click: None, extra: None }
    }

    pub fn ink(mut self, ink: Hsla) -> Self {
        self.ink = Some(ink);
        self
    }

    pub fn picked(mut self, picked: bool) -> Self {
        self.picked = picked;
        self
    }

    pub fn on_click(mut self, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(on_click));
        self
    }

    pub fn child(mut self, extra: impl IntoElement) -> Self {
        self.extra = Some(extra.into_any_element());
        self
    }
}

impl RenderOnce for PillButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let hovered = Hovered::of(keyed(&self.id, "hovered"), window, cx);
        let theme = Theme::of(cx);
        let ink = self.ink.unwrap_or(theme.fg);
        let fill = match (self.picked, hovered.is(cx)) {
            (true, _) => theme.selected_quiet(),
            (false, true) => theme.hover(),
            (false, false) => transparent_black(),
        };
        let fill = fade_color(keyed(&self.id, "fill"), fill, DURATION_FAST, window, cx);
        div()
            .id(self.id)
            .relative()
            .flex_none()
            .size(PILL_BUTTON_SIZE)
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(fill)
            .on_hover(hovered.track())
            .when_some(self.on_click, |button, clicked| {
                button.on_click(move |event, window, cx| clicked(event, window, cx))
            })
            .child(Icon::new(self.glyph).size(ICON_SIZE).color(ink))
            .children(self.extra)
    }
}
