use gpui::{
    App, Div, ElementId, InteractiveElement, Pixels, Point, ScrollDelta, ScrollEvent, ScrollHandle, Stateful,
    StatefulInteractiveElement, Styled, Window, div, point, px,
};

// GPUI on Wayland reports a wheel notch as 3 lines and multiplies a smooth delta by 3: https://github.com/ImperaLuna/zed/tree/47424cdc3f1a49829960e1ecbcdcdd967d034265/crates/gpui
const NOTCH_LINES: f32 = 3.0;
const NOTCH_PIXELS: f32 = 45.0;

pub const WHEEL_LINE: Pixels = px(24.0);

const GPUI_PIXEL_FACTOR: f32 = 3.0;

pub fn scrolled(event: &ScrollEvent) -> Point<Pixels> {
    match event.delta {
        ScrollDelta::Lines(lines) => point(WHEEL_LINE * lines.x, WHEEL_LINE * lines.y),
        ScrollDelta::Pixels(pixels) => point(pixels.x / GPUI_PIXEL_FACTOR, pixels.y / GPUI_PIXEL_FACTOR),
    }
}

pub fn wheel_list(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    let id = id.into();
    let handle = window.use_keyed_state(id.clone(), cx, |_, _| ScrollHandle::new()).read(cx).clone();
    let view = window.current_view();
    let wheeled = handle.clone();
    div().id(id).overflow_y_hidden().track_scroll(&handle).on_scroll(move |event, _, cx| {
        let offset = wheeled.offset();
        let moved = scrolled_list(offset, wheeled.max_offset(), scrolled(event));
        if moved != offset {
            wheeled.set_offset(moved);
            cx.notify(view);
        }
    })
}

fn scrolled_list(offset: Point<Pixels>, max: Point<Pixels>, by: Point<Pixels>) -> Point<Pixels> {
    let by = if by.y == px(0.0) { by.x } else { by.y };
    point(offset.x, (offset.y + by).clamp(-max.y, px(0.0)))
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Wheel {
    scrolled: f32,
}

impl Wheel {
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
        let swipe = scroll(ScrollDelta::Pixels(point(px(0.0), px(-30.0))));
        assert_eq!(scrolled(&swipe).y, px(-10.0));
    }

    #[test]
    fn a_list_scrolls_by_the_wheel_and_stops_at_both_ends() {
        let max = point(px(0.0), px(100.0));
        let down = point(px(0.0), px(-72.0));
        assert_eq!(scrolled_list(point(px(0.0), px(0.0)), max, down).y, px(-72.0));
        assert_eq!(scrolled_list(point(px(0.0), px(-72.0)), max, down).y, px(-100.0), "not past the end");
        assert_eq!(scrolled_list(point(px(0.0), px(-10.0)), max, -down).y, px(0.0), "nor past the start");
        assert_eq!(scrolled_list(point(px(0.0), px(0.0)), max, point(px(-30.0), px(0.0))).y, px(-30.0), "sideways too");
    }

    #[test]
    fn small_deltas_add_up_to_a_notch() {
        let mut wheel = Wheel::default();
        let tenth = scroll(ScrollDelta::Pixels(point(px(0.0), px(-NOTCH_PIXELS / 4.0))));
        let counted: Vec<i32> = (0..4).map(|_| wheel.notches(&tenth)).collect();
        assert_eq!(counted, [0, 0, 0, -1]);
    }
}
