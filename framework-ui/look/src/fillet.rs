//! The concave corner that joins glass to a screen edge: the power menu's tab
//! curves into the right edge above and below it. A square of the glass's tint
//! with a quarter circle cut from its corner away from the glass and the edge,
//! and the glass's outline along the curve.
//!
//! It takes clicks and blur like the glass beside it, all but its cutout:
//! whoever draws one adds [`Fillet::shapes`] to its window's shapes
//! (`crate::shape`), rows a pixel high that follow the curve.

use crate::Theme;
use crate::shape::Shape;
use crate::spinner::arc;
use crate::tokens::GLASS_EDGE_WIDTH;
use gpui::{App, Bounds, IntoElement, PathBuilder, Pixels, Point, RenderOnce, Styled, Window, canvas, point, px, size};
use std::f32::consts::FRAC_PI_2;

/// Which side of the glass a fillet sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilletSide {
    /// On top of the glass, curving up into the edge.
    Above,
    /// Below it, curving down.
    Below,
}

/// A fillet as an element that fills its parent, which its caller sizes
/// and places: a square, its right side on the screen edge.
#[derive(Debug, Clone, Copy, IntoElement)]
pub struct Fillet {
    side: FilletSide,
}

impl Fillet {
    pub fn new(side: FilletSide) -> Self {
        Self { side }
    }

    /// The fillet filling `bounds` as rows on whole pixels, for the
    /// window's blur and click regions: each runs from the curve to the
    /// edge, and rows that start equally far in are one.
    pub fn shapes(side: FilletSide, bounds: Bounds<Pixels>) -> Vec<Shape> {
        let left = f32::from(bounds.left()).round();
        let right = f32::from(bounds.right()).round();
        let top = f32::from(bounds.top()).round();
        let height = f32::from(bounds.bottom()).round() - top;
        let radius = right - left;
        let mut shapes: Vec<Shape> = Vec::new();
        let mut row = 0.0;
        while row < height {
            let inset = cut(side, radius, row);
            let mut band_end = row + 1.0;
            while band_end < height && cut(side, radius, band_end) == inset {
                band_end += 1.0;
            }
            let width = radius - inset;
            if width > 0.0 {
                let band = Bounds {
                    origin: point(px(left + inset), px(top + row)),
                    size: size(px(width), px(band_end - row)),
                };
                shapes.push(Shape::new(band, px(0.0)));
            }
            row = band_end;
        }
        shapes
    }
}

/// How far in from the fillet's left side its pixel row `row` starts: the
/// curve's reach at the row's middle, in whole pixels.
fn cut(side: FilletSide, radius: f32, row: f32) -> f32 {
    let middle = row + 0.5;
    // How far the row is from the circle's centre, up or down.
    let off_centre = match side {
        FilletSide::Above => middle,
        FilletSide::Below => radius - middle,
    };
    if off_centre >= radius {
        return 0.0;
    }
    (radius * radius - off_centre * off_centre).max(0.0).sqrt().round()
}

/// The curve from the corner it shares with the edge's far end to the one
/// it shares with the glass's, round the centre of the cut-out circle.
fn curve(side: FilletSide, bounds: Bounds<Pixels>) -> Vec<Point<Pixels>> {
    let radius = bounds.size.width;
    match side {
        // Centred on the top left corner: from the top right down to the
        // bottom left.
        FilletSide::Above => arc(bounds.origin, radius, 0.0, FRAC_PI_2),
        // Centred on the bottom left corner: from the bottom right up to
        // the top left.
        FilletSide::Below => arc(point(bounds.left(), bounds.bottom()), radius, 0.0, -FRAC_PI_2),
    }
}

impl RenderOnce for Fillet {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let (tint, outline) = (theme.glass(), theme.glass_outline());
        let side = self.side;
        canvas(
            |_, _, _| {},
            move |bounds: Bounds<Pixels>, (), window, _| {
                if bounds.size.width <= px(0.0) {
                    return;
                }
                let curve = curve(side, bounds);
                // The corner it shares with the glass and the edge, then
                // the curve back round.
                let corner = match side {
                    FilletSide::Above => [point(bounds.left(), bounds.bottom()), bounds.bottom_right()],
                    FilletSide::Below => [bounds.origin, bounds.top_right()],
                };
                let mut fill = PathBuilder::fill();
                fill.add_polygon(&[&corner[..], &curve[..]].concat(), true);
                if let Ok(path) = fill.build() {
                    window.paint_path(path, tint);
                }
                let mut line = PathBuilder::stroke(GLASS_EDGE_WIDTH);
                line.add_polygon(&curve, false);
                if let Ok(path) = line.build() {
                    window.paint_path(path, outline);
                }
            },
        )
        .size_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f32, y: f32, side: f32) -> Bounds<Pixels> {
        Bounds { origin: point(px(x), px(y)), size: size(px(side), px(side)) }
    }

    /// Whether the pixel at (`x`, `y`) is in any of `shapes`.
    fn covered(shapes: &[Shape], x: f32, y: f32) -> bool {
        shapes.iter().any(|shape| shape.bounds.contains(&point(px(x + 0.5), px(y + 0.5))))
    }

    #[test]
    fn above_it_fills_the_corner_by_the_glass_and_the_edge() {
        let shapes = Fillet::shapes(FilletSide::Above, square(100.0, 50.0, 22.0));
        assert!(covered(&shapes, 121.0, 71.0), "where the glass meets the edge");
        assert!(covered(&shapes, 121.0, 56.0), "up along the edge");
        assert!(covered(&shapes, 106.0, 71.0), "along the glass's top");
        assert!(!covered(&shapes, 100.0, 50.0), "the cut-out corner");
        assert!(!covered(&shapes, 108.0, 60.0), "inside the curve");
    }

    #[test]
    fn below_it_is_the_same_turned_over() {
        let above = Fillet::shapes(FilletSide::Above, square(0.0, 0.0, 22.0));
        let below = Fillet::shapes(FilletSide::Below, square(0.0, 0.0, 22.0));
        for y in 0..22 {
            for x in 0..22 {
                let (x, y) = (x as f32, y as f32);
                assert_eq!(covered(&above, x, y), covered(&below, x, 21.0 - y), "({x}, {y})");
            }
        }
    }

    #[test]
    fn each_row_reaches_from_the_curve_to_the_edge_and_matching_rows_are_one_shape() {
        let shapes = Fillet::shapes(FilletSide::Above, square(0.0, 0.0, 22.0));
        let widths: Vec<f32> = shapes.iter().map(|shape| f32::from(shape.bounds.size.width)).collect();
        assert!(widths.windows(2).all(|pair| pair[0] < pair[1]), "it widens row by row: {widths:?}");
        assert!(shapes.iter().all(|shape| f32::from(shape.bounds.right()) == 22.0), "each reaches the edge");
        for shape in &shapes {
            let (top, bottom) = (f32::from(shape.bounds.top()), f32::from(shape.bounds.bottom()));
            assert_eq!(bottom - top, f32::from(shape.bounds.size.height));
        }
        let rows: Vec<f32> = shapes.iter().map(|shape| f32::from(shape.bounds.top())).collect();
        assert!(rows.windows(2).all(|pair| pair[0] < pair[1]), "in order, none twice: {rows:?}");
        assert!(Fillet::shapes(FilletSide::Above, square(0.0, 0.0, 0.0)).is_empty());
    }
}
