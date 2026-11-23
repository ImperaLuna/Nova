//! Where things are on the stage: a panel's open box from its pill, the box
//! between the two while it grows, how tall the stage must be, the same for
//! a drawer against the right edge and its window, and whether a point is
//! on something drawn. Plain numbers, in screen coordinates, tested alone.

use gpui::{Bounds, Pixels, Point, Size, point, px, size};
use nova_look::Shape;
use nova_look::tokens::{BAR_GAP, BAR_HEIGHT, BAR_MARGIN, SHADOW_ROOM};

/// Which way a panel keeps to its pill: pills of the left row grow panels
/// that keep their left edge, and so on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Where a panel of `size` is once open: `top` down the screen, and across
/// it lined up with the pill it grows out of (`from`, with the way its row
/// keeps), or centered. A panel of the center row is centered on the
/// screen, not on its pill: the central pill sits off the middle when it
/// keeps its time there (the launcher opens in the middle, as today). It
/// never leaves the bar's margins.
pub fn open_box(
    from: Option<(Bounds<Pixels>, Align)>,
    size: Size<Pixels>,
    top: Pixels,
    screen_width: Pixels,
) -> Bounds<Pixels> {
    let left = match from {
        Some((pill, Align::Left)) => pill.left(),
        Some((pill, Align::Right)) => pill.right() - size.width,
        Some((_, Align::Center)) | None => (screen_width - size.width) / 2.0,
    };
    let furthest = (screen_width - BAR_MARGIN - size.width).max(BAR_MARGIN);
    Bounds { origin: point(left.clamp(BAR_MARGIN, furthest), top), size }
}

pub fn lerp(from: f32, to: f32, progress: f32) -> f32 {
    from + (to - from) * progress
}

fn lerp_px(from: Pixels, to: Pixels, progress: f32) -> Pixels {
    px(lerp(f32::from(from), f32::from(to), progress))
}

/// The box `progress` of the way (0..1) from `from` to `to`.
pub fn between(from: Bounds<Pixels>, to: Bounds<Pixels>, progress: f32) -> Bounds<Pixels> {
    Bounds {
        origin: point(lerp_px(from.origin.x, to.origin.x, progress), lerp_px(from.origin.y, to.origin.y, progress)),
        size: size(
            lerp_px(from.size.width, to.size.width, progress),
            lerp_px(from.size.height, to.size.height, progress),
        ),
    }
}

/// The radius `progress` of the way from a pill's to a panel's.
pub fn radius_between(panel_radius: Pixels, progress: f32) -> Pixels {
    lerp_px(BAR_HEIGHT / 2.0, panel_radius, progress.clamp(0.0, 1.0))
}

/// Whether two boxes share any pixels.
pub fn overlaps(one: Bounds<Pixels>, other: Bounds<Pixels>) -> bool {
    one.left() < other.right() && other.left() < one.right() && one.top() < other.bottom() && other.top() < one.bottom()
}

/// How tall the stage's window is: down to the lowest any of the panels
/// drawn on it reaches once open (`reaches`), and at least the bar's row,
/// plus the room a shadow needs. In whole pixels, since that is what a surface has.
pub fn stage_height(reaches: impl IntoIterator<Item = Pixels>) -> Pixels {
    let lowest = reaches.into_iter().fold(BAR_GAP + BAR_HEIGHT, Pixels::max);
    (lowest + SHADOW_ROOM).ceil()
}

/// Whether `at` is on one of `shapes`, corners left out: a click there is
/// not one outside the panels.
pub fn covered(shapes: &[Shape], at: Point<Pixels>) -> bool {
    shapes.iter().any(|shape| {
        let bounds = shape.bounds;
        if !bounds.contains(&at) {
            return false;
        }
        let radius = shape.radius.min(bounds.size.width / 2.0).min(bounds.size.height / 2.0);
        // How far into a corner's square the point is, on each axis.
        let across = (radius - (at.x - bounds.left()).min(bounds.right() - at.x)).max(px(0.0));
        let down = (radius - (at.y - bounds.top()).min(bounds.bottom() - at.y)).max(px(0.0));
        let (across, down, radius) = (f32::from(across), f32::from(down), f32::from(radius));
        across * across + down * down <= radius * radius
    })
}

/// Where a drawer against the right edge is once open: `width` wide, top
/// to bottom. Its content fills this.
pub fn edge_box(width: Pixels, screen: Size<Pixels>) -> Bounds<Pixels> {
    Bounds { origin: point(screen.width - width, px(0.0)), size: size(width, screen.height) }
}

/// The glass of a drawer that is open at `open`: its right corners
/// `radius` past the screen's edge, so only the left ones are round.
pub fn past_edge(open: Bounds<Pixels>, radius: Pixels) -> Bounds<Pixels> {
    Bounds { origin: open.origin, size: size(open.size.width + radius, open.size.height) }
}

/// What a drawer grows out of when its pill isn't on screen: the right end
/// of the bar's row, nothing wide. `QuickSettings.qml`'s `pill` in today's
/// shell.
pub fn edge_origin(screen_width: Pixels) -> Bounds<Pixels> {
    Bounds { origin: point(screen_width - BAR_MARGIN, BAR_GAP), size: size(px(0.0), BAR_HEIGHT) }
}

/// The size of the window a drawer `width` wide is drawn in: the drawer and
/// the room its shadow needs beside it, within the screen. In whole pixels.
pub fn edge_window(width: Pixels, screen: Size<Pixels>) -> Size<Pixels> {
    size((width + SHADOW_ROOM).ceil().min(screen.width), screen.height)
}

/// The size of the window for a drawer `width` wide while it grows out
/// of, or back into, a pill whose left edge is at `pill_left`: wide enough
/// for the pill too, when that is further from the edge than the drawer
/// reaches.
pub fn edge_window_from(width: Pixels, pill_left: Pixels, screen: Size<Pixels>) -> Size<Pixels> {
    edge_window(width.max(screen.width - pill_left), screen)
}

/// The part of a screen below a stage of `stage_height` and left of the
/// panels' window, which sits against the right edge and is `beside` wide
/// (nothing while none of its panels is out): where the click catcher
/// takes clicks, those two taking their own.
pub fn below_stage(stage_height: Pixels, beside: Pixels, screen: Size<Pixels>) -> Bounds<Pixels> {
    let height = (screen.height - stage_height).max(px(0.0));
    let width = (screen.width - beside).max(px(0.0));
    Bounds { origin: point(px(0.0), stage_height), size: size(width, height) }
}

/// The part of a stage `stage_height` tall left of the panels' window:
/// where the stage takes every click while a panel a click outside closes
/// is open, that window taking those on itself.
pub fn left_of_panels(stage_height: Pixels, beside: Pixels, screen_width: Pixels) -> Bounds<Pixels> {
    let width = (screen_width - beside).max(px(0.0));
    Bounds { origin: point(px(0.0), px(0.0)), size: size(width, stage_height) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Pixels = px(1920.0);

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds { origin: point(px(x), px(y)), size: size(px(width), px(height)) }
    }

    fn pill(x: f32, width: f32) -> Bounds<Pixels> {
        rect(x, 4.0, width, 36.0)
    }

    #[test]
    fn a_panel_keeps_to_its_pill_the_way_its_row_does() {
        let panel = size(px(300.0), px(400.0));
        let open = |pill, align| open_box(Some((pill, align)), panel, BAR_GAP, SCREEN);
        assert_eq!(open(pill(100.0, 120.0), Align::Left), rect(100.0, 4.0, 300.0, 400.0));
        assert_eq!(open(pill(900.0, 120.0), Align::Center), rect(810.0, 4.0, 300.0, 400.0));
        assert_eq!(open(pill(700.0, 120.0), Align::Center), rect(810.0, 4.0, 300.0, 400.0), "the screen's middle");
        assert_eq!(open(pill(1700.0, 120.0), Align::Right), rect(1520.0, 4.0, 300.0, 400.0));
    }

    #[test]
    fn a_panel_never_leaves_the_bar_s_margins() {
        let panel = size(px(300.0), px(400.0));
        let right_of_a_pill_at_the_edge = open_box(Some((pill(8.0, 60.0), Align::Right)), panel, BAR_GAP, SCREEN);
        assert_eq!(right_of_a_pill_at_the_edge.left(), BAR_MARGIN);
        let past_the_right = open_box(Some((pill(1800.0, 112.0), Align::Left)), panel, BAR_GAP, SCREEN);
        assert_eq!(past_the_right.right(), SCREEN - BAR_MARGIN);
    }

    #[test]
    fn a_panel_without_a_pill_is_centered_at_its_top() {
        let open = open_box(None, size(px(620.0), px(440.0)), px(60.0), SCREEN);
        assert_eq!(open, rect(650.0, 60.0, 620.0, 440.0));
    }

    #[test]
    fn the_morph_runs_from_the_pill_s_box_to_the_panel_s() {
        let (from, to) = (pill(900.0, 120.0), rect(810.0, 4.0, 300.0, 400.0));
        assert_eq!(between(from, to, 0.0), from);
        assert_eq!(between(from, to, 1.0), to);
        assert_eq!(between(from, to, 0.5), rect(855.0, 4.0, 210.0, 218.0));
        assert_eq!(radius_between(px(24.0), 0.0), px(18.0));
        assert_eq!(radius_between(px(24.0), 1.0), px(24.0));
    }

    #[test]
    fn the_stage_is_as_tall_as_its_tallest_panel_and_its_shadow() {
        assert_eq!(stage_height([]), px(80.0));
        assert_eq!(stage_height([px(404.0), px(500.5)]), px(541.0));
    }

    #[test]
    fn boxes_that_only_touch_do_not_overlap() {
        assert!(overlaps(pill(100.0, 120.0), rect(200.0, 4.0, 300.0, 400.0)));
        assert!(!overlaps(pill(100.0, 100.0), rect(200.0, 4.0, 300.0, 400.0)));
    }

    #[test]
    fn a_click_in_a_rounded_corner_is_outside() {
        let panel = [Shape::new(rect(100.0, 4.0, 300.0, 400.0), px(24.0))];
        assert!(covered(&panel, point(px(250.0), px(200.0))));
        assert!(covered(&panel, point(px(250.0), px(5.0))));
        assert!(!covered(&panel, point(px(101.0), px(5.0))), "the corner's cut-off part");
        assert!(covered(&panel, point(px(108.0), px(12.0))));
        assert!(!covered(&panel, point(px(99.0), px(200.0))));
        assert!(!covered(&[], point(px(0.0), px(0.0))));
    }

    #[test]
    fn the_catcher_takes_what_the_stage_does_not() {
        let screen = size(px(1920.0), px(1080.0));
        assert_eq!(below_stage(px(80.0), px(0.0), screen), rect(0.0, 80.0, 1920.0, 1000.0));
        assert_eq!(below_stage(px(2000.0), px(0.0), screen).size.height, px(0.0));
    }

    #[test]
    fn a_drawer_is_flush_with_the_right_edge_top_to_bottom() {
        let screen = size(px(1920.0), px(1080.0));
        let open = edge_box(px(368.0), screen);
        assert_eq!(open, rect(1552.0, 0.0, 368.0, 1080.0));
        assert_eq!(past_edge(open, px(24.0)), rect(1552.0, 0.0, 392.0, 1080.0), "its right corners off screen");
        assert_eq!(edge_origin(px(1920.0)), rect(1912.0, 4.0, 0.0, 36.0));
    }

    #[test]
    fn a_drawer_s_window_holds_it_and_its_shadow_and_no_more() {
        let screen = size(px(1920.0), px(1080.0));
        assert_eq!(edge_window(px(368.0), screen), size(px(408.0), px(1080.0)));
        assert_eq!(edge_window(px(367.5), screen).width, px(408.0), "whole pixels");
        assert_eq!(edge_window(px(3000.0), screen).width, px(1920.0), "never wider than the screen");
    }

    #[test]
    fn a_drawer_s_window_holds_the_pill_it_grows_out_of() {
        let screen = size(px(1920.0), px(1080.0));
        assert_eq!(edge_window_from(px(368.0), px(1800.0), screen).width, px(408.0), "a pill inside it");
        assert_eq!(edge_window_from(px(368.0), px(1400.0), screen).width, px(560.0), "one further left");
    }

    #[test]
    fn the_panels_window_takes_its_own_clicks() {
        let screen = size(px(1920.0), px(1080.0));
        assert_eq!(below_stage(px(80.0), px(408.0), screen), rect(0.0, 80.0, 1512.0, 1000.0));
        assert_eq!(left_of_panels(px(80.0), px(408.0), px(1920.0)), rect(0.0, 0.0, 1512.0, 80.0));
        assert_eq!(left_of_panels(px(80.0), px(0.0), px(1920.0)), rect(0.0, 0.0, 1920.0, 80.0));
        assert_eq!(left_of_panels(px(80.0), px(1920.0), px(1920.0)).size.width, px(0.0), "under a panel over it all");
    }
}
