use crate::tokens::ICON_SIZE_SMALL;
use crate::{Glyph, Icon, Theme};
use gpui::{
    App, ClickEvent, Div, ElementId, InteractiveElement, ParentElement, Pixels, Stateful, StatefulInteractiveElement,
    Styled, Window, div,
};

pub struct RoundButton {
    id: ElementId,
    glyph: Glyph,
    filled: bool,
    size: Pixels,
    on: bool,
    quiet: bool,
}

impl RoundButton {
    pub fn new(id: impl Into<ElementId>, glyph: Glyph, size: Pixels) -> Self {
        Self { id: id.into(), glyph, filled: false, size, on: false, quiet: false }
    }

    pub fn filled(mut self) -> Self {
        self.filled = true;
        self
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    pub fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }

    pub fn build(self, cx: &App, clicked: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Stateful<Div> {
        let theme = Theme::of(cx);
        let mut icon = Icon::new(self.glyph).size(ICON_SIZE_SMALL);
        if self.filled {
            icon = icon.filled(self.glyph);
        }
        let (fill, hover) = match (self.on, self.quiet) {
            (true, _) => (Some(theme.primary), theme.primary),
            (false, true) => (None, theme.hover()),
            (false, false) => (Some(theme.surface_raised()), theme.hover()),
        };
        let button = div()
            .id(self.id)
            .size(self.size)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .child(icon.color(if self.on { theme.on_accent } else { theme.fg }))
            .on_click(move |event, window, cx| {
                cx.stop_propagation();
                clicked(event, window, cx);
            });
        match fill {
            Some(fill) => button.bg(fill),
            None => button,
        }
    }
}
