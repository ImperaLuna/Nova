use gpui::{Bounds, Pixels, Point, Size, bounds, point, px, size};
use nova_recording_api::LayoutRect;
use nova_screenshot_api::PixelRect;
use std::time::{Duration, Instant};

pub fn region(from: Point<Pixels>, to: Point<Pixels>) -> Bounds<Pixels> {
    bounds(point(from.x.min(to.x), from.y.min(to.y)), size((to.x - from.x).abs(), (to.y - from.y).abs()))
}

pub fn is_click(region: Bounds<Pixels>, min_drag: Pixels) -> bool {
    region.size.width < min_drag || region.size.height < min_drag
}

pub fn to_pixels(region: Bounds<Pixels>, scale: f32, frame: Size<u32>) -> PixelRect {
    let edge = |value: Pixels, round: fn(f32) -> f32, most: u32| -> u32 {
        round(f32::from(value) * scale).clamp(0.0, most as f32) as u32
    };
    let left = edge(region.origin.x, f32::floor, frame.width);
    let top = edge(region.origin.y, f32::floor, frame.height);
    let right = edge(region.origin.x + region.size.width, f32::ceil, frame.width);
    let bottom = edge(region.origin.y + region.size.height, f32::ceil, frame.height);
    PixelRect { x: left, y: top, width: right.saturating_sub(left), height: bottom.saturating_sub(top) }
}

pub fn to_layout(region: Bounds<Pixels>, origin: Point<Pixels>) -> LayoutRect {
    let (x, y) = (f32::from(region.origin.x), f32::from(region.origin.y));
    let (left, top) = (x.ceil(), y.ceil());
    let width = ((x + f32::from(region.size.width)).floor() - left).max(0.0) as u32;
    let height = ((y + f32::from(region.size.height)).floor() - top).max(0.0) as u32;
    LayoutRect {
        x: (f32::from(origin.x) + left) as i32,
        y: (f32::from(origin.y) + top) as i32,
        width: width - width % 2,
        height: height - height % 2,
    }
}

pub fn clock(ms: u64) -> String {
    let total = ms / 1000;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 { format!("{hours}:{minutes:02}:{seconds:02}") } else { format!("{minutes}:{seconds:02}") }
}

pub fn marks(region: Bounds<Pixels>, gap: Pixels, length: Pixels, width: Pixels) -> [Bounds<Pixels>; 8] {
    let outer = region.dilate(gap);
    let (left, top) = (outer.origin.x, outer.origin.y);
    let (right, bottom) = (left + outer.size.width, top + outer.size.height);
    let arm = length + width;
    let across = |x: Pixels, y: Pixels| bounds(point(x, y), size(arm, width));
    let down = |x: Pixels, y: Pixels| bounds(point(x, y), size(width, arm));
    [
        across(left - width, top - width),
        down(left - width, top - width),
        across(right - length, top - width),
        down(right, top - width),
        across(left - width, bottom),
        down(left - width, bottom - length),
        across(right - length, bottom),
        down(right, bottom - length),
    ]
}

pub fn contain(picture: Size<u32>, room: Size<Pixels>) -> Size<Pixels> {
    if picture.width == 0 || picture.height == 0 {
        return size(px(0.0), px(0.0));
    }
    let scale = (f32::from(room.width) / picture.width as f32).min(f32::from(room.height) / picture.height as f32);
    size(px(picture.width as f32 * scale), px(picture.height as f32 * scale))
}

pub fn size_label(width: u32, height: u32) -> String {
    format!("{width} × {height}")
}

pub fn folder_label(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let end = parts.len().saturating_sub(1);
    parts[end.saturating_sub(2)..end].join("/")
}

pub fn under_region(region: Bounds<Pixels>, item: Size<Pixels>, screen: Size<Pixels>, gap: Pixels) -> Point<Pixels> {
    let x = (region.origin.x + region.size.width - item.width).max(px(0.0));
    let bottom = region.origin.y + region.size.height;
    let below = bottom + gap;
    let y = if below + item.height <= screen.height { below } else { (bottom - item.height - gap).max(px(0.0)) };
    point(x, y)
}

// GPUI rounds a box's top and height to device pixels separately, so boxes meeting on a fractional edge overlap:
// https://github.com/ImperaLuna/zed/blob/2033650502b1d0ec524b8913bf5a01b9463bed5d/crates/gpui/src/taffy.rs#L304-L306
pub fn on_pixels(region: Bounds<Pixels>, scale: f32) -> Bounds<Pixels> {
    let edge = |value: Pixels, round: fn(f32) -> f32| px(round(f32::from(value) * scale) / scale);
    let near = point(edge(region.origin.x, f32::floor), edge(region.origin.y, f32::floor));
    let far = point(
        edge(region.origin.x + region.size.width, f32::ceil),
        edge(region.origin.y + region.size.height, f32::ceil),
    );
    Bounds::from_corners(near, far)
}

pub fn around(clear: Bounds<Pixels>, screen: Size<Pixels>, scale: f32) -> [Bounds<Pixels>; 4] {
    let clear = if clear.is_empty() { clear } else { on_pixels(clear, scale) };
    let zero = px(0.0);
    let (top, bottom) = (clear.origin.y, clear.origin.y + clear.size.height);
    let (left, right) = (clear.origin.x, clear.origin.x + clear.size.width);
    let piece = |x: Pixels, y: Pixels, width: Pixels, height: Pixels| {
        bounds(point(x, y), size(width.max(zero), height.max(zero)))
    };
    [
        piece(zero, zero, screen.width, top),
        piece(zero, bottom, screen.width, screen.height - bottom),
        piece(zero, top, left, clear.size.height),
        piece(right, top, screen.width - right, clear.size.height),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Countdown {
    duration: Duration,
    spent: Duration,
    since: Option<Instant>,
}

impl Countdown {
    pub fn start(duration: Duration, now: Instant) -> Self {
        Self { duration, spent: Duration::ZERO, since: Some(now) }
    }

    pub fn set_held(&mut self, held: bool, now: Instant) {
        match (held, self.since) {
            (true, Some(since)) => {
                self.spent = (self.spent + now.saturating_duration_since(since)).min(self.duration);
                self.since = None;
            }
            (false, None) => self.since = Some(now),
            _ => {}
        }
    }

    pub fn held(&self) -> bool {
        self.since.is_none()
    }

    pub fn remaining(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 0.0;
        }
        let running = self.since.map_or(Duration::ZERO, |since| now.saturating_duration_since(since));
        let spent = (self.spent + running).min(self.duration);
        1.0 - spent.as_secs_f32() / self.duration.as_secs_f32()
    }

    pub fn running(&self, now: Instant) -> bool {
        self.since.is_some() && self.remaining(now) > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        bounds(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[test]
    fn a_drag_up_and_left_is_the_same_region_as_down_and_right() {
        let up = region(point(px(50.0), px(40.0)), point(px(10.0), px(10.0)));
        assert_eq!(up, rect(10.0, 10.0, 40.0, 30.0));
        assert_eq!(region(point(px(10.0), px(10.0)), point(px(50.0), px(40.0))), up);
    }

    #[test]
    fn a_region_thinner_than_the_least_drag_either_way_is_a_click() {
        assert!(is_click(rect(0.0, 0.0, 3.0, 100.0), px(4.0)));
        assert!(is_click(rect(0.0, 0.0, 100.0, 3.5), px(4.0)));
        assert!(!is_click(rect(0.0, 0.0, 4.0, 4.0), px(4.0)));
    }

    #[test]
    fn frame_pixels_round_outward_at_the_screens_scale_and_stay_on_the_frame() {
        let frame = size(2560, 1600);
        assert_eq!(
            to_pixels(rect(10.25, 20.5, 100.0, 50.0), 2.0, frame),
            PixelRect { x: 20, y: 41, width: 201, height: 100 }
        );
        assert_eq!(
            to_pixels(rect(-5.0, 1590.0, 1400.0, 40.0), 1.0, frame),
            PixelRect { x: 0, y: 1590, width: 1395, height: 10 }
        );
        assert_eq!(to_pixels(rect(3000.0, 0.0, 10.0, 10.0), 1.0, frame).width, 0);
    }

    #[test]
    fn a_shot_fits_the_card_whole() {
        let room = size(px(330.0), px(184.0));
        assert_eq!(contain(size(1280, 800), room), size(px(294.4), px(184.0)));
        assert_eq!(contain(size(660, 184), room), size(px(330.0), px(92.0)));
        assert_eq!(contain(size(0, 10), room), size(px(0.0), px(0.0)));
    }

    #[test]
    fn labels_read_as_the_card_says_them() {
        assert_eq!(size_label(960, 560), "960 × 560");
        assert_eq!(folder_label("/home/me/Pictures/Screenshots/shot.png"), "Pictures/Screenshots");
        assert_eq!(folder_label("/shot.png"), "");
        assert_eq!(folder_label("/tmp/shot.png"), "tmp");
    }

    #[test]
    fn the_badge_goes_inside_the_region_when_it_would_leave_the_screen() {
        let screen = size(px(1000.0), px(800.0));
        let badge = size(px(80.0), px(26.0));
        let low = under_region(rect(100.0, 600.0, 200.0, 190.0), badge, screen, px(6.0));
        assert_eq!(low, point(px(220.0), px(758.0)));
        let high = under_region(rect(10.0, 10.0, 50.0, 50.0), badge, screen, px(6.0));
        assert_eq!(high, point(px(0.0), px(66.0)));
    }

    #[test]
    fn the_veil_covers_all_but_the_clear_region() {
        let screen = size(px(100.0), px(80.0));
        let [top, bottom, left, right] = around(rect(10.0, 20.0, 30.0, 40.0), screen, 1.0);
        assert_eq!(top, rect(0.0, 0.0, 100.0, 20.0));
        assert_eq!(bottom, rect(0.0, 60.0, 100.0, 20.0));
        assert_eq!(left, rect(0.0, 20.0, 10.0, 40.0));
        assert_eq!(right, rect(40.0, 20.0, 60.0, 40.0));
        let none = around(Bounds::default(), screen, 1.0);
        assert_eq!(none[1], rect(0.0, 0.0, 100.0, 80.0));
    }

    #[test]
    fn veil_pieces_meet_on_whole_device_pixels_at_a_fractional_scale() {
        let scale = 1.25;
        let screen = size(px(2048.0), px(1280.0));
        let [top, bottom, left, right] = around(rect(100.3, 200.1, 968.9, 768.7), screen, scale);
        let device = |value: Pixels| f32::from(value) * scale;
        let whole = |value: Pixels| (device(value) - device(value).round()).abs() < 1e-3;
        for piece in [top, bottom, left, right] {
            assert!(whole(piece.origin.x) && whole(piece.origin.y), "{piece:?}");
            assert!(whole(piece.size.width) && whole(piece.size.height), "{piece:?}");
        }
        assert_eq!(right.origin.y + right.size.height, bottom.origin.y);
        assert_eq!(top.origin.y + top.size.height, right.origin.y);
        let saved = to_pixels(rect(100.3, 200.1, 968.9, 768.7), scale, size(2560, 1600));
        assert_eq!(device(bottom.origin.y).round() as u32, saved.y + saved.height);
        assert_eq!(device(right.origin.x).round() as u32, saved.x + saved.width);
    }

    #[test]
    fn a_recorded_region_is_on_the_layout_and_even() {
        let origin = point(px(1920.0), px(0.0));
        assert_eq!(
            to_layout(rect(10.5, 20.0, 101.0, 51.9), origin),
            LayoutRect { x: 1931, y: 20, width: 100, height: 50 }
        );
        assert_eq!(to_layout(rect(0.0, 0.0, 1.0, 1.0), point(px(0.0), px(0.0))).width, 0);
    }

    #[test]
    fn a_recording_s_length_reads_as_a_clock() {
        assert_eq!(clock(7_900), "0:07");
        assert_eq!(clock(754_000), "12:34");
        assert_eq!(clock(3_723_000), "1:02:03");
    }

    #[test]
    fn the_marks_sit_outside_the_region_s_corners() {
        let region = rect(100.0, 100.0, 200.0, 100.0);
        let marks = marks(region, px(4.0), px(18.0), px(3.0));
        assert_eq!(marks[0], rect(93.0, 93.0, 21.0, 3.0));
        assert_eq!(marks[1], rect(93.0, 93.0, 3.0, 21.0));
        assert_eq!(marks[6], rect(286.0, 204.0, 21.0, 3.0));
        assert_eq!(marks[7], rect(304.0, 186.0, 3.0, 21.0));
        assert!(marks.iter().all(|mark| !mark.intersects(&region)));
    }

    #[test]
    fn a_held_countdown_keeps_its_time() {
        let start = Instant::now();
        let mut countdown = Countdown::start(Duration::from_secs(6), start);
        assert!((countdown.remaining(start + Duration::from_secs(3)) - 0.5).abs() < 1e-4);
        countdown.set_held(true, start + Duration::from_secs(3));
        assert!(!countdown.running(start + Duration::from_secs(60)));
        assert!((countdown.remaining(start + Duration::from_secs(60)) - 0.5).abs() < 1e-4);
        assert!(countdown.held());
        countdown.set_held(false, start + Duration::from_secs(60));
        assert!(countdown.running(start + Duration::from_secs(62)));
        assert_eq!(countdown.remaining(start + Duration::from_secs(63)), 0.0);
        assert!(!countdown.running(start + Duration::from_secs(63)));
    }
}
