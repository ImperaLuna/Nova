//! An app's icon: the image file novad found for it, else the app's first
//! letter on a swatch picked by its id, so a missing icon still looks
//! deliberate. The launcher's rows and tiles and a notification's card
//! draw it.
//!
//! novad looks icon names up in the icon theme and sends an absolute path
//! (`nova_app_info::App::icon`). GPUI reads and decodes the file off the
//! main thread and keeps the picture, so an icon costs its file once; the
//! letter shows until then, and stays when the file can't be read.

use crate::Theme;
use crate::tokens::{APP_ICON_SIZE, LETTER_SCALE, LETTER_TILE_RADIUS, WEIGHT_TITLE};
use gpui::{
    AnyElement, App, Hsla, IntoElement, ObjectFit, ParentElement, Pixels, RenderOnce, SharedString, Styled,
    StyledImage, Window, div, img,
};
use std::path::PathBuf;

/// An app's icon, square (see the module).
#[derive(Debug, Clone, IntoElement)]
pub struct AppIcon {
    id: SharedString,
    name: SharedString,
    icon: Option<SharedString>,
    size: Pixels,
}

impl AppIcon {
    /// The icon of the app `id` (its desktop file id, or whatever names it
    /// for good) called `name`.
    pub fn new(id: impl Into<SharedString>, name: impl Into<SharedString>) -> Self {
        Self { id: id.into(), name: name.into(), icon: None, size: APP_ICON_SIZE }
    }

    /// The absolute path of its image, as novad sends it. Anything else,
    /// like an icon's name, draws the letter.
    pub fn icon(mut self, icon: Option<impl Into<SharedString>>) -> Self {
        self.icon = icon.map(Into::into).filter(|icon| icon.starts_with('/'));
        self
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for AppIcon {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let swatches = &Theme::of(cx).swatches;
        let tile = LetterTile {
            letter: letter(&self.name).into(),
            swatch: swatches[swatch(&self.id, swatches.len())],
            ink: Theme::of(cx).on_accent,
            size: self.size,
        };
        let Some(icon) = self.icon else {
            return tile.build();
        };
        let (loading, missing) = (tile.clone(), tile);
        img(PathBuf::from(icon.as_ref()))
            .size(self.size)
            .flex_none()
            .object_fit(ObjectFit::Contain)
            .with_loading(move || loading.clone().build())
            .with_fallback(move || missing.clone().build())
            .into_any_element()
    }
}

/// An app's letter on its swatch.
#[derive(Clone)]
struct LetterTile {
    letter: SharedString,
    swatch: Hsla,
    ink: Hsla,
    size: Pixels,
}

impl LetterTile {
    fn build(self) -> AnyElement {
        // As round as a row's icon is, at any size.
        let radius = LETTER_TILE_RADIUS * (self.size / APP_ICON_SIZE);
        div()
            .size(self.size)
            .flex_none()
            .rounded(radius)
            .bg(self.swatch)
            .flex()
            .items_center()
            .justify_center()
            .text_size((self.size * LETTER_SCALE).round())
            .font_weight(WEIGHT_TITLE)
            .text_color(self.ink)
            .child(self.letter)
            .into_any_element()
    }
}

/// The letter an app without an icon shows: its name's first letter or
/// digit.
fn letter(name: &str) -> String {
    let first = name.chars().find(|character| character.is_alphanumeric());
    first.map_or_else(|| "?".into(), |character| character.to_uppercase().collect())
}

/// Which of `swatches` swatches an app's letter sits on: the same for the
/// same id every time, and the one today's shell picks
/// (`ui/launcher/launcher.js`).
fn swatch(id: &str, swatches: usize) -> usize {
    // JavaScript's `(hash * 31 + code) | 0` over UTF-16 code units.
    let hash = id.encode_utf16().fold(0i32, |hash, unit| hash.wrapping_mul(31).wrapping_add(i32::from(unit)));
    hash.unsigned_abs() as usize % swatches.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_icon_shows_the_name_s_first_letter_or_digit() {
        assert_eq!(letter("  firefox"), "F");
        assert_eq!(letter("1Password"), "1");
        assert_eq!(letter("ßeta"), "SS");
        assert_eq!(letter("***"), "?");
    }

    #[test]
    fn an_app_keeps_its_swatch_and_it_is_today_s() {
        // launcher.js: swatch("a") is 2 and swatch("firefox.desktop") is 8, counted from 1.
        assert_eq!(swatch("a", 8), 1);
        assert_eq!(swatch("firefox.desktop", 8), 7);
        assert_eq!(swatch("", 8), 0);
    }

    #[test]
    fn only_a_path_is_an_icon() {
        assert!(AppIcon::new("a", "A").icon(Some("/usr/share/icons/a.png")).icon.is_some());
        assert!(AppIcon::new("a", "A").icon(Some("firefox")).icon.is_none());
        assert!(AppIcon::new("a", "A").icon(None::<String>).icon.is_none());
    }
}
