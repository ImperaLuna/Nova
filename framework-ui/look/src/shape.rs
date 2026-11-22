//! The shape of what a shell window draws, as the compositor needs it.
//!
//! A shell window is see-through and much bigger than what's on it, so the
//! compositor is told which part is really there: where to blur what's
//! behind (glass), and where clicks land (everything else passes them to
//! the window below). Both are regions made of whole-pixel rectangles, and
//! both must follow the drawing exactly, rounded corners too: a blur that
//! covers the whole window costs the compositor frames, and one that pokes
//! out past a rounded corner shows.
//!
//! A [`Shape`] is one rounded rectangle; [`Shape::rects`] cuts it into rows
//! that follow its corners a pixel at a time, and [`region`] joins several.
//! The window's owner works its shapes out while it renders, from the same
//! bounds and radius it draws with, and passes them to [`set_blur_region`]
//! and to GPUI's `Window::set_input_region` every frame they change.

use gpui::{Bounds, Pixels, Window, point, px, size};

/// A rounded rectangle on a window, in the window's own coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    pub bounds: Bounds<Pixels>,
    pub radius: Pixels,
}

impl Shape {
    pub fn new(bounds: Bounds<Pixels>, radius: Pixels) -> Self {
        Self { bounds, radius }
    }

    /// The shape as rectangles on whole pixels: one for its middle, and
    /// rows above and below that step in along the corners, a pixel row at
    /// a time (rows that step in equally are one rectangle).
    pub fn rects(&self) -> Vec<Bounds<Pixels>> {
        // Whole pixels, since that is what a region holds: the edges
        // rounded, not the origin and the size apart, so a shape that moves
        // by fractions never grows or shrinks by a pixel.
        let left = f32::from(self.bounds.left()).round();
        let right = f32::from(self.bounds.right()).round();
        let top = f32::from(self.bounds.top()).round();
        let bottom = f32::from(self.bounds.bottom()).round();
        let (width, height) = (right - left, bottom - top);
        if width <= 0.0 || height <= 0.0 {
            return Vec::new();
        }
        let radius = f32::from(self.radius).clamp(0.0, width.min(height) / 2.0);
        let corner_rows = radius.ceil();

        let mut rects = Vec::new();
        let mut row = 0.0;
        while row < corner_rows {
            // Rows that step in by the same whole pixels form one band.
            let inset = corner_inset(radius, row);
            let mut band_end = row + 1.0;
            while band_end < corner_rows && corner_inset(radius, band_end) == inset {
                band_end += 1.0;
            }
            let (band_width, band_height) = (width - 2.0 * inset, band_end - row);
            if band_width > 0.0 {
                rects.push(rect(left + inset, top + row, band_width, band_height));
                rects.push(rect(left + inset, bottom - band_end, band_width, band_height));
            }
            row = band_end;
        }
        let middle_height = height - 2.0 * corner_rows;
        if middle_height > 0.0 {
            rects.push(rect(left, top + corner_rows, width, middle_height));
        }
        rects
    }
}

/// How far a corner of `radius` steps in on pixel row `row` (0 is the
/// shape's first row), in whole pixels: the arc's inset at the row's middle.
fn corner_inset(radius: f32, row: f32) -> f32 {
    let from_centre = radius - (row + 0.5);
    if from_centre <= 0.0 {
        return 0.0;
    }
    (radius - (radius * radius - from_centre * from_centre).max(0.0).sqrt()).round()
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
    Bounds { origin: point(px(x), px(y)), size: size(px(width), px(height)) }
}

/// Every shape's rectangles, as one region.
pub fn region(shapes: &[Shape]) -> Vec<Bounds<Pixels>> {
    shapes.iter().flat_map(Shape::rects).collect()
}

/// Has the compositor blur what's behind `shapes` and nothing else of the
/// window. With no shapes nothing is blurred. A shell window opens blurred
/// all over (`crate::ShellWindow`), so its owner calls this before its
/// first frame; GPUI drops a call that changes nothing.
pub fn set_blur_region(window: &Window, shapes: &[Shape]) {
    window.set_blur_region(Some(&region(shapes)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(x: f32, y: f32, width: f32, height: f32, radius: f32) -> Shape {
        Shape::new(rect(x, y, width, height), px(radius))
    }

    fn edges(rect: &Bounds<Pixels>) -> [f32; 4] {
        [rect.left(), rect.top(), rect.right(), rect.bottom()].map(f32::from)
    }

    fn area(rects: &[Bounds<Pixels>]) -> f32 {
        rects.iter().map(|rect| f32::from(rect.size.width) * f32::from(rect.size.height)).sum()
    }

    /// Whether the pixel at (`x`, `y`) is in any of `rects`.
    fn covered(rects: &[Bounds<Pixels>], x: f32, y: f32) -> usize {
        rects
            .iter()
            .filter(|rect| {
                let [left, top, right, bottom] = edges(rect);
                x >= left && x < right && y >= top && y < bottom
            })
            .count()
    }

    #[test]
    fn a_square_cornered_shape_is_one_rectangle() {
        assert_eq!(shape(10.0, 20.0, 300.0, 200.0, 0.0).rects(), vec![rect(10.0, 20.0, 300.0, 200.0)]);
    }

    #[test]
    fn a_panel_s_region_follows_its_corners_within_a_pixel() {
        let (width, height, radius) = (620.0, 440.0, 24.0);
        let rects = shape(100.0, 8.0, width, height, radius).rects();
        for rect in &rects {
            let [left, top, right, bottom] = edges(rect);
            assert!(left >= 100.0 && right <= 100.0 + width && top >= 8.0 && bottom <= 8.0 + height, "{rect:?}");
            assert_eq!([left, top, right, bottom].map(f32::fract), [0.0; 4], "{rect:?} is off the pixel grid");
        }
        // No pixel twice, and each pixel in the region exactly when its
        // middle is inside the rounded rectangle.
        for y in 0..height as u32 {
            for x in 0..width as u32 {
                let (x, y) = (x as f32, y as f32);
                let (dx, dy) = ((x + 0.5 - width / 2.0).abs(), (y + 0.5 - height / 2.0).abs());
                let (cx, cy) = (dx - (width / 2.0 - radius), dy - (height / 2.0 - radius));
                let inside = cx <= 0.0 || cy <= 0.0 || (cx * cx + cy * cy).sqrt() <= radius + 0.75;
                let outside = cx > 0.0 && cy > 0.0 && (cx * cx + cy * cy).sqrt() >= radius - 0.75;
                let count = covered(&rects, 100.0 + x, 8.0 + y);
                assert!(count <= 1, "pixel {x},{y} is in {count} rectangles");
                assert!(count == 1 || outside, "pixel {x},{y} is inside the shape but not in the region");
                assert!(count == 0 || inside, "pixel {x},{y} is outside the shape but in the region");
            }
        }
        let exact = width * height - (4.0 - std::f32::consts::PI) * radius * radius;
        assert!((area(&rects) - exact).abs() < 0.01 * exact, "{} against {exact}", area(&rects));
    }

    #[test]
    fn a_pill_is_all_corner() {
        // As tall as twice its radius: no straight middle part.
        let rects = shape(0.0, 0.0, 200.0, 36.0, 18.0).rects();
        assert_eq!(covered(&rects, 0.0, 0.0), 0);
        assert_eq!(covered(&rects, 0.0, 17.0), 1);
        assert_eq!(covered(&rects, 100.0, 0.0), 1);
        assert_eq!(covered(&rects, 199.0, 35.0), 0);
        let exact = 200.0 * 36.0 - (4.0 - std::f32::consts::PI) * 18.0 * 18.0;
        assert!((area(&rects) - exact).abs() < 0.01 * exact, "{} against {exact}", area(&rects));
    }

    #[test]
    fn a_radius_past_half_the_shape_is_cut_to_it() {
        assert_eq!(shape(0.0, 0.0, 200.0, 36.0, 500.0).rects(), shape(0.0, 0.0, 200.0, 36.0, 18.0).rects());
    }

    #[test]
    fn a_shape_between_pixels_keeps_its_edges_not_its_size() {
        // 10.4..110.6 covers pixels 10 to 111: rounding origin and width
        // apart would give 10..110.
        let rects = shape(10.4, 0.0, 100.2, 50.0, 0.0).rects();
        assert_eq!(rects, vec![rect(10.0, 0.0, 101.0, 50.0)]);
    }

    #[test]
    fn nothing_comes_of_an_empty_shape() {
        assert_eq!(shape(5.0, 5.0, 0.0, 40.0, 8.0).rects(), Vec::new());
        assert_eq!(shape(5.0, 5.0, 40.0, -3.0, 8.0).rects(), Vec::new());
        assert_eq!(region(&[]), Vec::new());
    }

    #[test]
    fn a_region_is_every_shape_s_rectangles() {
        let (pill, panel) = (shape(0.0, 0.0, 200.0, 36.0, 18.0), shape(300.0, 0.0, 300.0, 200.0, 24.0));
        let joined = region(&[pill, panel]);
        assert_eq!(joined.len(), pill.rects().len() + panel.rects().len());
        assert_eq!(covered(&joined, 100.0, 18.0), 1);
        assert_eq!(covered(&joined, 450.0, 100.0), 1);
        assert_eq!(covered(&joined, 250.0, 18.0), 0);
    }
}
