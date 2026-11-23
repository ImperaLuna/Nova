//! A mouse wheel's notches, out of GPUI's scroll events: one notch steps a
//! workspace or a volume level, and a touchpad's small deltas add up to
//! one. And how far a wheel scrolls a list ([`scrolled`]).

use gpui::{Pixels, Point, ScrollDelta, ScrollEvent, point, px};

/// One notch as GPUI reports it on Wayland: a wheel's click is three lines
/// (GPUI's `SCROLL_LINES`), and as a smooth delta (a wheel that also sends
/// one, or a touchpad) 15 of the compositor's units times GPUI's factor 3.
const NOTCH_LINES: f32 = 3.0;
const NOTCH_PIXELS: f32 = 45.0;

/// How far a list scrolls for each line a wheel turns: Qt's Flickable
/// moves 24 px a line and three lines a notch (`QStyleHints`'
/// `wheelScrollLines`), 72 px, whatever its rows' height. GPUI calls the
/// same notch three lines too.
pub const WHEEL_LINE: Pixels = px(24.0);

/// What GPUI multiplies a smooth delta (a touchpad's) by on Wayland. Qt
/// scrolls a list by the compositor's own units.
const GPUI_PIXEL_FACTOR: f32 = 3.0;

/// How far `event` scrolls a list, up and left above zero, as far as
/// today's lists go: a wheel's lines at [`WHEEL_LINE`], a touchpad's
/// movement as the compositor sent it.
pub fn scrolled(event: &ScrollEvent) -> Point<Pixels> {
    match event.delta {
        ScrollDelta::Lines(lines) => point(WHEEL_LINE * lines.x, WHEEL_LINE * lines.y),
        ScrollDelta::Pixels(pixels) => point(pixels.x / GPUI_PIXEL_FACTOR, pixels.y / GPUI_PIXEL_FACTOR),
    }
}

/// What a view counts a wheel's notches with, while the pointer stays.
#[derive(Debug, Default, Clone, Copy)]
pub struct Wheel {
    /// Notches scrolled and not stepped yet: below one either way.
    scrolled: f32,
}

impl Wheel {
    /// Adds `event` and returns the whole notches it completes: below zero
    /// for each one scrolled down (or right), above for up (or left), like
    /// Qt's `angleDelta`. Vertical scrolling wins over horizontal.
    pub fn notches(&mut self, event: &ScrollEvent) -> i32 {
        let (y, x) = match event.delta {
            ScrollDelta::Lines(lines) => (lines.y / NOTCH_LINES, lines.x / NOTCH_LINES),
            ScrollDelta::Pixels(pixels) => (pixels.y / px(NOTCH_PIXELS), pixels.x / px(NOTCH_PIXELS)),
        };
        self.scrolled += if y == 0.0 { x } else { y };
        let whole = self.scrolled.trunc();
        self.scrolled -= whole;
        whole as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scroll(delta: ScrollDelta) -> ScrollEvent {
        ScrollEvent { delta, ..Default::default() }
    }

    #[test]
    fn a_wheel_click_is_one_notch_down() {
        let mut wheel = Wheel::default();
        assert_eq!(wheel.notches(&scroll(ScrollDelta::Lines(point(0.0, -NOTCH_LINES)))), -1);
        assert_eq!(wheel.notches(&scroll(ScrollDelta::Lines(point(0.0, 2.0 * NOTCH_LINES)))), 2);
    }

    #[test]
    fn a_wheel_notch_scrolls_a_list_as_far_as_qt_does() {
        let notch = scroll(ScrollDelta::Lines(point(0.0, -NOTCH_LINES)));
        assert_eq!(scrolled(&notch).y, px(-72.0));
        // A touchpad's 10 units, which GPUI hands over as 30 pixels.
        let swipe = scroll(ScrollDelta::Pixels(point(px(0.0), px(-30.0))));
        assert_eq!(scrolled(&swipe).y, px(-10.0));
    }

    #[test]
    fn small_deltas_add_up_to_a_notch() {
        let mut wheel = Wheel::default();
        let tenth = scroll(ScrollDelta::Pixels(point(px(0.0), px(-NOTCH_PIXELS / 4.0))));
        let counted: Vec<i32> = (0..4).map(|_| wheel.notches(&tenth)).collect();
        assert_eq!(counted, [0, 0, 0, -1]);
    }
}
