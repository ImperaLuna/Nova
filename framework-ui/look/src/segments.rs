//! A row of tabs, one of them picked: the launcher's Apps and Files, a
//! page's sections. `ui/theme/Segments.qml` in today's shell.
//!
//! [`Segments`] draws the options side by side, as wide as each other, in
//! the width its parent gives it. It keeps no state: the owner says which
//! key is picked and gets the click.

use crate::Theme;
use crate::tokens::{
    FONT_SIZE_SMALL, RADIUS_ROW, RADIUS_SEGMENT, SEGMENT_HEIGHT, SEGMENT_PAD, WEIGHT_BODY, WEIGHT_TITLE,
};
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
    on_pick: Option<Picked>,
}

impl Segments {
    /// Tabs for `options`, each a key and its label, with `current` picked.
    pub fn new<Key: Into<SharedString>, Label: Into<SharedString>>(
        options: impl IntoIterator<Item = (Key, Label)>,
        current: impl Into<SharedString>,
    ) -> Self {
        let options = options.into_iter().map(|(key, label)| (key.into(), label.into())).collect();
        Self { options, current: current.into(), filled: true, on_pick: None }
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
        let options = self.options.into_iter().map(|(key, label)| {
            let picked = key == self.current;
            let on_pick = self.on_pick.clone();
            let option = div()
                .flex_1()
                .h(SEGMENT_HEIGHT)
                .rounded(RADIUS_SEGMENT)
                .flex()
                .items_center()
                .justify_center()
                .text_size(FONT_SIZE_SMALL)
                .font_weight(if picked { WEIGHT_TITLE } else { WEIGHT_BODY })
                .cursor(CursorStyle::PointingHand)
                .child(label)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    if let Some(on_pick) = &on_pick {
                        on_pick(&key, window, cx);
                    }
                });
            if picked { option.bg(picked_fill) } else { option.hover(move |style| style.bg(hover)) }
        });
        let row = div().flex().items_center().gap(SEGMENT_PAD).p(SEGMENT_PAD).rounded(RADIUS_ROW).children(options);
        if self.filled { row.bg(fill) } else { row }
    }
}
