use crate::images::{Pictures, Wanted};
use crate::tokens::DURATION_PREVIEW;
use gpui::{
    AnyElement, App, AppContext, Context, DevicePixels, Entity, IntoElement, ObjectFit, ParentElement, Size, Styled,
    StyledImage, div, img,
};
use nova_look::motion::Eased;
use std::time::Instant;

const KEEPS: usize = 2;

pub struct Preview {
    pictures: Entity<Pictures>,
    front: Option<Wanted>,
    back: Option<Wanted>,
    fade: Eased,
    fading: bool,
}

impl Preview {
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
        if self.fading {
            self.front = self.back.take();
            self.fading = false;
        }
        self.back = Some(wanted);
        self.fade = Eased::lasting(0.0, DURATION_PREVIEW, now);
    }

    pub fn moving(&self, now: Instant) -> bool {
        self.fading && !self.fade.settled(now)
    }

    pub fn release(&self, window: Option<&mut gpui::Window>, cx: &mut App) {
        self.pictures.update(cx, |pictures, cx| pictures.release(window, cx));
    }

    pub fn element(&mut self, now: Instant, cx: &mut App) -> AnyElement {
        let back = self.back.clone();
        let back_picture =
            back.as_ref().and_then(|wanted| self.pictures.update(cx, |pictures, cx| pictures.get(wanted, cx)));
        if back_picture.is_some() && !self.fading {
            self.fading = true;
            self.fade.set_target(1.0, now);
        }
        if self.fading && self.fade.settled(now) {
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
