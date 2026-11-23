//! A toggle tile in quick settings: an icon and a title, filled with the accent
//! while on. The fill says its state, so no line under the title does. With an
//! arrow at its end, a click there opens the tile's page instead of toggling.
//!
//! [`Tile::empty`] is a tile's place with nothing in it yet: quick settings
//! keeps it for a feature whose tile isn't drawn here yet.

use crate::tokens::{
    DIM_OPACITY, DURATION_COLOR, FONT_SIZE, GAP, ICON_SIZE_MENU, LABEL_NUDGE, PAD, RADIUS_CARD, ROW_INSET,
    TILE_ARROW_WIDTH, TILE_HEIGHT, WEIGHT_TITLE,
};
use crate::{Glyph, Hovered, Icon, Theme, fade_color, keyed};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div,
};
use std::rc::Rc;

type Clicked = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A toggle tile (see the module).
#[derive(IntoElement)]
pub struct Tile {
    id: ElementId,
    glyph: Option<Glyph>,
    fill: Option<Glyph>,
    title: SharedString,
    on: bool,
    available: bool,
    on_toggle: Option<Clicked>,
    on_open: Option<Clicked>,
}

impl Tile {
    pub fn new(id: impl Into<ElementId>, glyph: Glyph, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            glyph: Some(glyph),
            fill: None,
            title: title.into(),
            on: false,
            available: true,
            on_toggle: None,
            on_open: None,
        }
    }

    /// A tile's place with nothing in it: the raised fill alone.
    pub fn empty(id: impl Into<ElementId>) -> Self {
        Self { glyph: None, available: false, ..Self::new(id, Glyph::STAR, "") }
    }

    /// The icon's solid parts.
    pub fn filled(mut self, fill: Glyph) -> Self {
        self.fill = Some(fill);
        self
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    /// `false` dims it and takes no clicks (the mic with no input).
    pub fn available(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    pub fn on_toggle(mut self, toggled: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(toggled));
        self
    }

    /// Gives it the arrow, which opens its page.
    pub fn on_open(mut self, opened: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_open = Some(Rc::new(opened));
        self
    }
}

impl RenderOnce for Tile {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (available, has_glyph) = (self.available, self.glyph.is_some());
        // The fill fades between the accent, the hover wash and the raised
        // fill, as today's does.
        let hovered = Hovered::of(keyed(&self.id, "hovered"), window, cx);
        let pointed = available && hovered.is(cx);
        let theme = Theme::of(cx).clone();
        let ink = if self.on { theme.on_accent } else { theme.fg };
        let fill = match (self.on, pointed) {
            (true, _) => theme.primary,
            (false, true) => theme.hover(),
            (false, false) => theme.surface_raised(),
        };
        let fill = fade_color(keyed(&self.id, "fill"), fill, DURATION_COLOR, window, cx);
        let arrow = self.on_open.map(|opened| {
            div()
                .id("arrow")
                .flex_none()
                .w(TILE_ARROW_WIDTH + GAP)
                .mr(LABEL_NUDGE * 2.0)
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(Glyph::RIGHT).color(ink))
                .when(available, |arrow| {
                    arrow.on_click(move |event, window, cx| {
                        cx.stop_propagation();
                        opened(event, window, cx);
                    })
                })
        });
        let has_arrow = arrow.is_some();
        div()
            .id(self.id)
            .w_full()
            .h(TILE_HEIGHT)
            .rounded(RADIUS_CARD)
            .flex()
            .items_center()
            .pl(PAD + LABEL_NUDGE)
            .when(!has_arrow, |tile| tile.pr(PAD))
            .gap(ROW_INSET)
            .bg(fill)
            .on_hover(hovered.track())
            .when(!available && has_glyph, |tile| tile.opacity(DIM_OPACITY))
            .when_some(self.on_toggle.filter(|_| available), |tile, toggled| {
                tile.on_click(move |event, window, cx| toggled(event, window, cx))
            })
            .when_some(self.glyph, |tile, glyph| {
                let icon = Icon::new(glyph).size(ICON_SIZE_MENU).color(ink);
                tile.child(match self.fill {
                    Some(fill) => icon.filled(fill),
                    None => icon,
                })
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(FONT_SIZE)
                    .font_weight(WEIGHT_TITLE)
                    .text_color(ink)
                    .child(self.title),
            )
            .children(arrow)
    }
}
