//! A row of tabs, one of them picked: the launcher's Apps and Files, a
//! page's sections. `ui/theme/Segments.qml` in today's shell.
//!
//! [`Segments`] draws the options side by side, as wide as each other, in
//! the width its parent gives it, some with an icon before the label. It
//! keeps no state: the owner says which key is picked and gets the click.
//! [`Segments::accent`] fills the picked one with the accent, for a choice
//! that changes the machine (the network page's Prefer) rather than what
//! the page shows.

use crate::tokens::{
    FONT_SIZE_SMALL, ICON_GAP, ICON_SIZE_SMALL, RADIUS_ROW, RADIUS_SEGMENT, SEGMENT_HEIGHT, SEGMENT_PAD, WEIGHT_BODY,
    WEIGHT_TITLE,
};
use crate::{Glyph, Icon, Theme};
use gpui::{
    App, CursorStyle, InteractiveElement, IntoElement, MouseButton, ParentElement, RenderOnce, SharedString, Styled,
    Window, div,
};
use std::rc::Rc;

type Picked = Rc<dyn Fn(&SharedString, &mut Window, &mut App)>;

/// A row of tabs (see the module).
#[derive(IntoElement)]
pub struct Segments {
    options: Vec<(SharedString, SharedString)>,
    current: SharedString,
    filled: bool,
    accent: bool,
    /// Icons before some labels, by key: a glyph and its solid parts.
    icons: Vec<(SharedString, Glyph, Option<Glyph>)>,
    on_pick: Option<Picked>,
}

impl Segments {
    /// Tabs for `options`, each a key and its label, with `current` picked.
    pub fn new<Key: Into<SharedString>, Label: Into<SharedString>>(
        options: impl IntoIterator<Item = (Key, Label)>,
        current: impl Into<SharedString>,
    ) -> Self {
        let options = options.into_iter().map(|(key, label)| (key.into(), label.into())).collect();
        Self { options, current: current.into(), filled: true, accent: false, icons: Vec::new(), on_pick: None }
    }

    /// The picked one filled with the accent.
    pub fn accent(mut self, accent: bool) -> Self {
        self.accent = accent;
        self
    }

    /// `glyph` (with `fill` solid under it) before the label of `key`.
    pub fn icon(mut self, key: impl Into<SharedString>, glyph: Glyph, fill: Option<Glyph>) -> Self {
        self.icons.push((key.into(), glyph, fill));
        self
    }

    /// `false` for tabs that sit on something raised already (inside a
    /// field): the row draws no fill of its own.
    pub fn filled(mut self, filled: bool) -> Self {
        self.filled = filled;
        self
    }

    /// Called with an option's key when it is clicked.
    pub fn on_pick(mut self, on_pick: impl Fn(&SharedString, &mut Window, &mut App) + 'static) -> Self {
        self.on_pick = Some(Rc::new(on_pick));
        self
    }
}

impl RenderOnce for Segments {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let (hover, picked_fill, fill) = (theme.hover(), theme.selected_quiet(), theme.surface_raised());
        let (accent, icons) = (self.accent, self.icons);
        let options = self.options.into_iter().map(|(key, label)| {
            let picked = key == self.current;
            let ink = if picked && accent { theme.on_accent } else { theme.fg };
            let icon = icons.iter().find(|(each, ..)| *each == key).map(|(_, glyph, fill)| {
                let icon = Icon::new(*glyph).size(ICON_SIZE_SMALL).color(ink);
                match fill {
                    Some(fill) => icon.filled(*fill),
                    None => icon,
                }
            });
            let on_pick = self.on_pick.clone();
            let option = div()
                .flex_1()
                .h(SEGMENT_HEIGHT)
                .rounded(RADIUS_SEGMENT)
                .flex()
                .items_center()
                .justify_center()
                .gap(ICON_GAP)
                .text_size(FONT_SIZE_SMALL)
                .font_weight(if picked { WEIGHT_TITLE } else { WEIGHT_BODY })
                .text_color(ink)
                .cursor(CursorStyle::PointingHand)
                .children(icon)
                .child(label)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    if let Some(on_pick) = &on_pick {
                        on_pick(&key, window, cx);
                    }
                });
            match (picked, accent) {
                (true, true) => option.bg(theme.primary),
                (true, false) => option.bg(picked_fill),
                (false, _) => option.hover(move |style| style.bg(hover)),
            }
        });
        let row = div().flex().items_center().gap(SEGMENT_PAD).p(SEGMENT_PAD).rounded(RADIUS_ROW).children(options);
        if self.filled { row.bg(fill) } else { row }
    }
}
