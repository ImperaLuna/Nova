//! A picture over the whole screen that fades to the next once that has
//! loaded, so stepping through images never shows a blank; moving on
//! before one has loaded loads the newest instead. `PreviewImage.qml`.
//! The carousel draws one under its deck, and each other screen's preview
//! window draws one.

use crate::images::{Pictures, Wanted};
use crate::tokens::DURATION_PREVIEW;
use gpui::{
    AnyElement, App, AppContext, Context, DevicePixels, Entity, IntoElement, ObjectFit, ParentElement, Size, Styled,
    StyledImage, div, img,
};
use nova_look::motion::Eased;
use std::time::Instant;

/// How many screen-sized pictures it holds: the one shown and the next.
const KEEPS: usize = 2;

pub struct Preview {
    pictures: Entity<Pictures>,
    /// The picture showing, and the one fading in over it.
    front: Option<Wanted>,
    back: Option<Wanted>,
    /// The back one's opacity, while it fades in.
    fade: Eased,
    fading: bool,
}

impl Preview {
    /// A preview whose pictures, as they arrive, redraw the view `cx`
    /// belongs to.
    pub fn new<V: 'static>(cx: &mut Context<V>) -> Self {
        let pictures = cx.new(|_| Pictures::new(KEEPS));
        cx.observe(&pictures, |_, _, cx| cx.notify()).detach();
        Self {
            pictures,
            front: None,
            back: None,
            fade: Eased::lasting(0.0, DURATION_PREVIEW, Instant::now()),
            fading: false,
        }
    }

    /// Shows `path` ("" for none) decoded at `pixels`, the screen's size.
    pub fn show(&mut self, path: &str, pixels: Size<DevicePixels>, cx: &mut App) {
        let now = Instant::now();
        if path.is_empty() {
            self.front = None;
            self.back = None;
            self.fading = false;
            self.pictures.update(cx, |pictures, _| pictures.clear());
            return;
        }
        let wanted = Wanted::new(path, pixels);
        if self.front.as_ref() == Some(&wanted) || self.back.as_ref() == Some(&wanted) {
            if self.front.as_ref() == Some(&wanted) && !self.fading {
                self.back = None;
            }
            return;
        }
        // A fade cut short: the one fading in is the front now, in full.
        if self.fading {
            self.front = self.back.take();
            self.fading = false;
        }
        self.back = Some(wanted);
        self.fade = Eased::lasting(0.0, DURATION_PREVIEW, now);
    }

    /// Whether it needs more frames.
    pub fn moving(&self, now: Instant) -> bool {
        self.fading && !self.fade.settled(now)
    }

    /// Takes the pictures it let go out of the GPU's atlas (see
    /// [`Pictures::release`]).
    pub fn release(&self, window: Option<&mut gpui::Window>, cx: &mut App) {
        self.pictures.update(cx, |pictures, cx| pictures.release(window, cx));
    }

    /// The picture as it is at `now`, filling its parent.
    pub fn element(&mut self, now: Instant, cx: &mut App) -> AnyElement {
        let back = self.back.clone();
        let back_picture =
            back.as_ref().and_then(|wanted| self.pictures.update(cx, |pictures, cx| pictures.get(wanted, cx)));
        if back_picture.is_some() && !self.fading {
            self.fading = true;
            self.fade.set_target(1.0, now);
        }
        if self.fading && self.fade.settled(now) {
            // Faded in: the back is the front, and the old picture goes.
            self.front = self.back.take();
            self.fading = false;
        }
        let front = self.front.clone();
        let front_picture =
            front.as_ref().and_then(|wanted| self.pictures.update(cx, |pictures, cx| pictures.get(wanted, cx)));
        let layer = |picture: std::sync::Arc<gpui::RenderImage>, opacity: f32| {
            img(picture).absolute().inset_0().size_full().object_fit(ObjectFit::Cover).opacity(opacity)
        };
        let back_layer = self
            .fading
            .then(|| {
                self.back.as_ref().and_then(|wanted| self.pictures.update(cx, |pictures, cx| pictures.get(wanted, cx)))
            })
            .flatten()
            .map(|picture| layer(picture, self.fade.value(now)));
        div()
            .absolute()
            .inset_0()
            .children(front_picture.map(|picture| layer(picture, 1.0)))
            .children(back_layer)
            .into_any_element()
    }
}
