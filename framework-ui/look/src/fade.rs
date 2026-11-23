//! Short fades that today's shell writes as a QML `Behavior` with a plain
//! `ColorAnimation` or `NumberAnimation`: a hover wash, a tile's fill
//! turning the accent. They run on a straight line over their duration, as
//! Qt's do by default, from wherever they are when the target changes.
//!
//! An element that isn't a view of its own ([`crate::Tile`], a row) keeps
//! them by its id in the window ([`gpui::Window::use_keyed_state`]), so they
//! last from one frame to the next. Each asks for frames only while it
//! moves. Whether the pointer is on an element comes from its `on_hover`,
//! which GPUI tells after a frame: [`Hovered`] keeps it for the next one.

use crate::mix;
use gpui::{App, ElementId, Entity, Hsla, Window};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Where a fade between two colors is.
struct ColorFade {
    from: Hsla,
    to: Hsla,
    started: Instant,
    duration: Duration,
}

impl ColorFade {
    fn at(&self, now: Instant) -> Hsla {
        mix(self.from, self.to, progress(self.started, self.duration, now))
    }

    fn settled(&self, now: Instant) -> bool {
        self.from == self.to || progress(self.started, self.duration, now) >= 1.0
    }

    fn head_for(&mut self, target: Hsla, now: Instant) {
        if target != self.to {
            self.from = self.at(now);
            self.to = target;
            self.started = now;
        }
    }
}

/// How far a straight fade that `started` and lasts `duration` is at `now`.
fn progress(started: Instant, duration: Duration, now: Instant) -> f32 {
    if duration.is_zero() {
        return 1.0;
    }
    (now.saturating_duration_since(started).as_secs_f32() / duration.as_secs_f32()).min(1.0)
}

/// An id for something an element keeps (`name`), under the element's own.
pub fn keyed(id: &ElementId, name: &'static str) -> ElementId {
    ElementId::NamedChild(Arc::new(id.clone()), name.into())
}

/// The color the element `id` shows now, fading to `target` over
/// `duration` whenever `target` changes. It starts at `target`.
pub fn fade_color(
    id: impl Into<ElementId>,
    target: Hsla,
    duration: Duration,
    window: &mut Window,
    cx: &mut App,
) -> Hsla {
    let now = Instant::now();
    let fade =
        window.use_keyed_state(id.into(), cx, |_, _| ColorFade { from: target, to: target, started: now, duration });
    fade.update(cx, |fade, _| fade.head_for(target, now));
    let fade = fade.read(cx);
    if !fade.settled(now) {
        window.request_animation_frame();
    }
    fade.at(now)
}

/// A number the element `id` shows now (an opacity), fading to `target`
/// over `duration` like [`fade_color`].
pub fn fade(id: impl Into<ElementId>, target: f32, duration: Duration, window: &mut Window, cx: &mut App) -> f32 {
    let as_color = |value: f32| Hsla { h: 0.0, s: 0.0, l: 0.0, a: value };
    fade_color(id, as_color(target.clamp(0.0, 1.0)), duration, window, cx).a
}

/// Whether the pointer is on an element, as its last `on_hover` said.
#[derive(Clone)]
pub struct Hovered(Entity<bool>);

impl Hovered {
    /// The element `id`'s state, kept in the window while it is drawn.
    pub fn of(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> Self {
        Self(window.use_keyed_state(id.into(), cx, |_, _| false))
    }

    pub fn is(&self, cx: &App) -> bool {
        *self.0.read(cx)
    }

    /// What goes in the element's `on_hover`: keeps the news, and redraws
    /// the window when it changed (this frame read it).
    pub fn track(&self) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
        let state = self.0.clone();
        move |hovered, _, cx| {
            state.update(cx, |was, cx| {
                if *was != *hovered {
                    *was = *hovered;
                    cx.notify();
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::hsla;

    #[test]
    fn a_fade_runs_straight_from_where_it_is() {
        let start = Instant::now();
        let (black, white) = (hsla(0.0, 0.0, 0.0, 1.0), hsla(0.0, 0.0, 1.0, 1.0));
        let mut fade = ColorFade { from: black, to: black, started: start, duration: Duration::from_millis(100) };
        fade.head_for(white, start);
        let half = fade.at(start + Duration::from_millis(50));
        assert!((half.l - 0.5).abs() < 0.01, "{half:?}");
        assert!(!fade.settled(start + Duration::from_millis(50)));
        // Turned round half-way: back from the middle, not from white.
        fade.head_for(black, start + Duration::from_millis(50));
        assert!((fade.at(start + Duration::from_millis(50)).l - 0.5).abs() < 0.01);
        assert!(fade.settled(start + Duration::from_millis(150)));
        assert_eq!(fade.at(start + Duration::from_millis(150)), black);
    }
}
