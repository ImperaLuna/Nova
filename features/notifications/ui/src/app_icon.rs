//! An app's icon, `size` square: the image file novad names once it
//! loads, else the app's first letter on a swatch picked by its id, so a
//! missing icon still looks deliberate. Today's `ui/launcher/AppIcon.qml`.
//!
//! The launcher's port brings the shared one to nova-look (IMP-399); this
//! one goes once that lands.

use crate::logic::{letter, swatch};
use crate::tokens::{LETTER_SCALE, LETTER_TILE_FOR, LETTER_TILE_RADIUS};
use gpui::{
    App, IntoElement, ObjectFit, ParentElement, Pixels, RenderOnce, SharedString, Styled, StyledImage, Window, div, img,
};
use nova_look::Theme;
use nova_look::tokens::WEIGHT_TITLE;
use std::path::PathBuf;

#[derive(IntoElement)]
pub struct AppIcon {
    /// What picks its swatch: the desktop file id, else the name.
    id: SharedString,
    name: SharedString,
    icon: Option<PathBuf>,
    size: Pixels,
}

impl AppIcon {
    pub fn new(id: impl Into<SharedString>, name: impl Into<SharedString>, size: Pixels) -> Self {
        Self { id: id.into(), name: name.into(), icon: None, size }
    }

    /// The icon's file, an absolute path.
    pub fn icon(mut self, icon: Option<&str>) -> Self {
        self.icon = icon.filter(|path| path.starts_with('/')).map(PathBuf::from);
        self
    }
}

impl RenderOnce for AppIcon {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let size = self.size;
        let (tint, ink) = (theme.swatches[swatch(&self.id, theme.swatches.len())], theme.on_accent);
        let letter = SharedString::from(letter(&self.name));
        // Built again for each of the image's states: an element is drawn once.
        let tile = move || {
            div()
                .size(size)
                .flex()
                .items_center()
                .justify_center()
                .rounded(LETTER_TILE_RADIUS * (size / LETTER_TILE_FOR))
                .bg(tint)
                .text_size(size * LETTER_SCALE)
                .font_weight(WEIGHT_TITLE)
                .text_color(ink)
                .child(letter.clone())
                .into_any_element()
        };
        let Some(icon) = self.icon else { return tile() };
        img(icon)
            .size(size)
            .object_fit(ObjectFit::Contain)
            .with_loading(tile.clone())
            .with_fallback(tile)
            .into_any_element()
    }
}
