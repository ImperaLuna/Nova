use crate::Theme;
use crate::tokens::{APP_ICON_SIZE, LETTER_SCALE, LETTER_TILE_RADIUS, WEIGHT_TITLE};
use gpui::{
    AnyElement, App, Hsla, IntoElement, ObjectFit, ParentElement, Pixels, RenderOnce, SharedString, Styled,
    StyledImage, Window, div, img,
};
use std::path::PathBuf;

#[derive(Debug, Clone, IntoElement)]
pub struct AppIcon {
    id: SharedString,
    name: SharedString,
    icon: Option<SharedString>,
    size: Pixels,
}

impl AppIcon {
    pub fn new(id: impl Into<SharedString>, name: impl Into<SharedString>) -> Self {
        Self { id: id.into(), name: name.into(), icon: None, size: APP_ICON_SIZE }
    }

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

#[derive(Clone)]
struct LetterTile {
    letter: SharedString,
    swatch: Hsla,
    ink: Hsla,
    size: Pixels,
}

impl LetterTile {
    fn build(self) -> AnyElement {
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

fn letter(name: &str) -> String {
    let first = name.chars().find(|character| character.is_alphanumeric());
    first.map_or_else(|| "?".into(), |character| character.to_uppercase().collect())
}

fn swatch(id: &str, swatches: usize) -> usize {
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
    fn an_app_keeps_its_swatch() {
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
