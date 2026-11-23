//! A card turned about its upright middle line and seen in perspective, as
//! Qt Quick draws today's cards (`Rotation` about the y axis in
//! `WallpaperCarousel.qml`): the side turned away gets smaller, the side
//! turned to you bigger. Only numbers here: where a point of the card
//! lands, which point of the card is under a point of the screen, and the
//! box the turned card fills. `card.wgsl` draws it with the same sums.
//!
//! Everything is in pixels from the card's middle; the card is already at
//! its drawn size (Qt scales an item before it turns it).

/// How far the eye is from the screen, in pixels: Qt Quick's own distance
/// for a `Rotation`, which is what gives today's cards their perspective.
pub const EYE: f32 = 1024.0;

/// A card of a size, turned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tilt {
    /// Half the card's width and height.
    pub half_width: f32,
    pub half_height: f32,
    /// The cosine of the turn: how much narrower the card gets.
    pub narrow: f32,
    /// How fast a point moves away from the eye with its distance from the
    /// middle line (the turn's sine over the eye's distance); below zero
    /// when the left side is the one turned away.
    pub lean: f32,
}

impl Tilt {
    /// A card `width` by `height`, turned by `degrees`: above zero its
    /// right side goes away from you.
    pub fn new(width: f32, height: f32, degrees: f32) -> Self {
        let turn = degrees.to_radians();
        Self { half_width: width / 2.0, half_height: height / 2.0, narrow: turn.cos(), lean: turn.sin() / EYE }
    }

    /// Where the card's point lands on the screen.
    pub fn onto_screen(&self, x: f32, y: f32) -> (f32, f32) {
        let depth = 1.0 + self.lean * x;
        (self.narrow * x / depth, y / depth)
    }

    /// The card's point under a point of the screen; `None` past where the
    /// card's plane can be seen at all.
    pub fn onto_card(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let reach = self.narrow - self.lean * x;
        if reach <= f32::EPSILON {
            return None;
        }
        let on_card = x / reach;
        Some((on_card, y * (1.0 + self.lean * on_card)))
    }

    /// Whether a point of the screen is on the card grown by `margin`.
    pub fn covers(&self, x: f32, y: f32, margin: f32) -> bool {
        self.onto_card(x, y)
            .is_some_and(|(x, y)| x.abs() <= self.half_width + margin && y.abs() <= self.half_height + margin)
    }

    /// The box the card grown by `margin` fills on the screen: its left,
    /// top, right and bottom.
    pub fn reach(&self, margin: f32) -> [f32; 4] {
        let (x, y) = (self.half_width + margin, self.half_height + margin);
        let corners = [(-x, -y), (x, -y), (-x, y), (x, y)].map(|(x, y)| self.onto_screen(x, y));
        let over = |pick: fn(f32, f32) -> f32, start: f32, side: fn(&(f32, f32)) -> f32| {
            corners.iter().map(side).fold(start, pick)
        };
        [
            over(f32::min, f32::INFINITY, |corner| corner.0),
            over(f32::min, f32::INFINITY, |corner| corner.1),
            over(f32::max, f32::NEG_INFINITY, |corner| corner.0),
            over(f32::max, f32::NEG_INFINITY, |corner| corner.1),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn a_card_facing_you_is_drawn_as_it_is() {
        let flat = Tilt::new(400.0, 200.0, 0.0);
        assert_eq!(flat.onto_screen(120.0, -80.0), (120.0, -80.0));
        assert_eq!(flat.reach(3.0), [-203.0, -103.0, 203.0, 103.0]);
    }

    #[test]
    fn the_side_turned_away_is_smaller_and_the_other_bigger() {
        let turned = Tilt::new(400.0, 200.0, 50.0);
        let (far_x, far_y) = turned.onto_screen(200.0, 100.0);
        let (close_x, close_y) = turned.onto_screen(-200.0, 100.0);
        // Qt's sums: x cos / (1 + x sin / 1024), y / (1 + x sin / 1024).
        assert!(near(far_x, 111.83) && near(far_y, 86.99), "{far_x} {far_y}");
        assert!(near(close_x, -151.18) && near(close_y, 117.60), "{close_x} {close_y}");
        // Turned the other way, it mirrors.
        let (x, y) = Tilt::new(400.0, 200.0, -50.0).onto_screen(-200.0, 100.0);
        assert!(near(x, -far_x) && near(y, far_y));
    }

    #[test]
    fn a_point_of_the_screen_finds_its_point_of_the_card() {
        let turned = Tilt::new(400.0, 200.0, -50.0);
        for (x, y) in [(0.0, 0.0), (200.0, 100.0), (-200.0, -100.0), (63.0, -12.5)] {
            let (on_x, on_y) = turned.onto_screen(x, y);
            let (back_x, back_y) = turned.onto_card(on_x, on_y).unwrap();
            assert!(near(back_x, x) && near(back_y, y), "{x} {y} came back as {back_x} {back_y}");
        }
    }

    #[test]
    fn the_box_holds_the_tall_side_and_the_slanted_edges_leave_its_corners_out() {
        let turned = Tilt::new(400.0, 200.0, 50.0);
        let [left, top, right, bottom] = turned.reach(0.0);
        assert!(near(left, -151.18) && near(right, 111.83) && near(top, -117.60) && near(bottom, 117.60));
        // The box's corner on the small side is above the card's edge.
        assert!(!turned.covers(right - 1.0, top + 1.0, 0.0));
        assert!(turned.covers(left + 1.0, top + 1.0, 0.0));
        assert!(turned.covers(0.0, 0.0, 0.0));
    }
}
