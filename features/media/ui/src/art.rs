//! A picture in a rounded box: the image at a path once it loads, else a
//! music note on a quiet square. For the app's icon (not
//! backed: no box behind the picture) and the track's cover in the panel
//! (backed, and cut to the box's rounding).

use gpui::{AnyElement, App, IntoElement, ObjectFit, ParentElement, Pixels, Styled, StyledImage, div, img};
use nova_look::{Glyph, Icon, Theme};
use std::path::PathBuf;

/// The art's box: how big, how big the picture is in it, how round, and
/// whether the box shows behind a picture.
#[derive(Debug, Clone, Copy)]
pub struct Art {
    pub size: Pixels,
    pub picture: Pixels,
    pub radius: Pixels,
    pub backed: bool,
}

impl Art {
    /// An app's icon: round, with no box behind it once it loads.
    pub fn icon(size: Pixels, picture: Pixels) -> Self {
        Self { size, picture, radius: size / 2.0, backed: false }
    }

    /// A cover: it fills its box and takes its rounding.
    pub fn cover(size: Pixels, radius: Pixels) -> Self {
        Self { size, picture: size, radius, backed: true }
    }

    /// The art for the image at `path` (absolute), or the note without one.
    pub fn build(self, path: Option<&str>, cx: &App) -> AnyElement {
        let theme = Theme::of(cx);
        let (raised, dim) = (theme.surface_raised(), theme.fg_dim());
        let note = move || {
            div()
                .size(self.size)
                .flex_none()
                .rounded(self.radius)
                .bg(raised)
                .flex()
                .items_center()
                .justify_center()
                .child(Icon::new(Glyph::MUSIC).size((self.size / 2.0).round()).color(dim))
                .into_any_element()
        };
        let Some(path) = path.filter(|path| path.starts_with('/')) else {
            return note();
        };
        let picture = img(PathBuf::from(path)).size(self.picture).flex_none().with_loading(note).with_fallback(note);
        let picture = if self.backed {
            picture.rounded(self.radius).object_fit(ObjectFit::Cover)
        } else {
            picture.object_fit(ObjectFit::Contain)
        };
        let frame = div().size(self.size).flex_none().flex().items_center().justify_center();
        let frame = if self.backed { frame.rounded(self.radius).bg(raised) } else { frame };
        frame.child(picture).into_any_element()
    }
}
