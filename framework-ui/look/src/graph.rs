//! A graph over time: a history's values as a line, newest at the right,
//! and the area under one. The network page's traffic and the resources
//! page draw with it. `graphPath` in `ui/quick_settings/quick_settings.js`
//! in today's shell.
//!
//! [`points`] places the values, with no GPUI drawing in it; [`line`] and
//! [`area`] paint them from inside a `canvas`.

use crate::tokens::GRAPH_LINE;
use gpui::{Bounds, Hsla, PathBuilder, Pixels, Point, Window, point};

/// The most a value is: values are shares of the graph's height, in percent.
pub const FULL: f32 = 100.0;

/// Where each of `values` (0 to [`FULL`], oldest first) sits in `bounds`:
/// spaced so that `span` of them fill the width, the newest at the right
/// edge, a value of 0 on the bottom edge. Values past either end are held
/// to it; a history longer than `span` keeps its newest.
pub fn points(values: &[f32], bounds: Bounds<Pixels>, span: usize) -> Vec<Point<Pixels>> {
    let values = &values[values.len().saturating_sub(span.max(1))..];
    let step = bounds.size.width / span.saturating_sub(1).max(1) as f32;
    let start = bounds.size.width - step * values.len().saturating_sub(1) as f32;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let x = start + step * index as f32;
            let y = bounds.size.height - bounds.size.height * (value.clamp(0.0, FULL) / FULL);
            point(bounds.origin.x + x, bounds.origin.y + y)
        })
        .collect()
}

/// Strokes `points` as one line in `color`.
pub fn line(points: &[Point<Pixels>], color: Hsla, window: &mut Window) {
    if points.len() < 2 {
        return;
    }
    let mut path = PathBuilder::stroke(GRAPH_LINE);
    path.add_polygon(points, false);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// Fills between `points` and the bottom of `bounds` in `color`.
pub fn area(points: &[Point<Pixels>], bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let (Some(first), Some(last)) = (points.first(), points.last()) else { return };
    let bottom = bounds.bottom();
    let mut outline = points.to_vec();
    outline.push(point(last.x, bottom));
    outline.push(point(first.x, bottom));
    let mut path = PathBuilder::fill();
    path.add_polygon(&outline, true);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{px, size};

    fn bounds() -> Bounds<Pixels> {
        Bounds { origin: point(px(10.0), px(20.0)), size: size(px(100.0), px(50.0)) }
    }

    #[test]
    fn the_newest_value_is_at_the_right_edge() {
        let placed = points(&[0.0, 50.0, 100.0], bounds(), 11);
        assert_eq!(placed.len(), 3);
        assert_eq!(placed[2], point(px(110.0), px(20.0)), "full, at the top right");
        assert_eq!(placed[1], point(px(100.0), px(45.0)), "half, one step left");
        assert_eq!(placed[0], point(px(90.0), px(70.0)), "nothing, on the bottom");
    }

    #[test]
    fn a_longer_history_keeps_its_newest_span() {
        let placed = points(&[10.0, 20.0, 0.0, 100.0], bounds(), 2);
        assert_eq!(placed, [point(px(10.0), px(70.0)), point(px(110.0), px(20.0))]);
    }

    #[test]
    fn a_full_history_spans_the_width_and_values_stay_inside() {
        let placed = points(&[-5.0, 250.0], bounds(), 2);
        assert_eq!(placed[0], point(px(10.0), px(70.0)));
        assert_eq!(placed[1], point(px(110.0), px(20.0)));
        assert!(points(&[], bounds(), 60).is_empty());
    }
}
