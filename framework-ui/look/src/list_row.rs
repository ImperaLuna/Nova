//! A list row in a quick settings page: an icon, a name with a line under
//! it, and a status at its end ("In use"), quietly filled while selected.
//! `ui/quick_settings/ListRow.qml` in today's shell.

use crate::tokens::{FONT_SIZE, FONT_SIZE_SMALL, GAP, ICON_SIZE_MENU, LIST_ROW_HEIGHT, PAD, RADIUS_ROW, WEIGHT_LABEL};
use crate::{Glyph, Icon, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};
use std::rc::Rc;

type Clicked = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A list row (see the module).
#[derive(IntoElement)]
pub struct ListRow {
    id: ElementId,
    glyph: Glyph,
    title: SharedString,
    subtitle: SharedString,
    status: SharedString,
    selected: bool,
    on_click: Option<Clicked>,
}

impl ListRow {
    pub fn new(id: impl Into<ElementId>, glyph: Glyph, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            glyph,
            title: title.into(),
            subtitle: SharedString::default(),
            status: SharedString::default(),
            selected: false,
            on_click: None,
        }
    }

    /// The line under the name; none when empty.
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = subtitle.into();
        self
    }

    /// The word at its end, in the accent while selected.
    pub fn status(mut self, status: impl Into<SharedString>) -> Self {
        self.status = status.into();
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn on_click(mut self, clicked: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(clicked));
        self
    }
}

impl RenderOnce for ListRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let hover = theme.hover();
        let status_ink = if self.selected { theme.primary } else { theme.fg_dim() };
        div()
            .id(self.id)
            .flex_none()
            .w_full()
            .h(LIST_ROW_HEIGHT)
            .px(PAD)
            .rounded(RADIUS_ROW)
            .flex()
            .items_center()
            .gap(PAD)
            .when(self.selected, |row| row.bg(theme.surface_raised()))
            .hover(move |style| style.bg(hover))
            .when_some(self.on_click, |row, clicked| row.on_click(move |event, window, cx| clicked(event, window, cx)))
            .child(Icon::new(self.glyph).size(ICON_SIZE_MENU))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .mr(GAP - PAD)
                    .flex()
                    .flex_col()
                    .child(div().truncate().text_size(FONT_SIZE).font_weight(WEIGHT_LABEL).child(self.title))
                    .when(!self.subtitle.is_empty(), |text| {
                        text.child(
                            div().truncate().text_size(FONT_SIZE_SMALL).text_color(theme.fg_dim()).child(self.subtitle),
                        )
                    }),
            )
            .child(div().flex_none().text_size(FONT_SIZE_SMALL).text_color(status_ink).child(self.status))
    }
}
