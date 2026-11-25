pub const EYE: f32 = 1024.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tilt {
    pub half_width: f32,
    pub half_height: f32,
    pub narrow: f32,
    pub lean: f32,
}

impl Tilt {
    pub fn new(width: f32, height: f32, degrees: f32) -> Self {
        let turn = degrees.to_radians();
        Self { half_width: width / 2.0, half_height: height / 2.0, narrow: turn.cos(), lean: turn.sin() / EYE }
    }

    pub fn onto_screen(&self, x: f32, y: f32) -> (f32, f32) {
        let depth = 1.0 + self.lean * x;
        (self.narrow * x / depth, y / depth)
    }

    pub fn onto_card(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let reach = self.narrow - self.lean * x;
        if reach <= f32::EPSILON {
            return None;
        }
        let on_card = x / reach;
        Some((on_card, y * (1.0 + self.lean * on_card)))
    }

    pub fn covers(&self, x: f32, y: f32, margin: f32) -> bool {
        self.onto_card(x, y)
            .is_some_and(|(x, y)| x.abs() <= self.half_width + margin && y.abs() <= self.half_height + margin)
    }

    pub fn hidden_under(
        &self,
        middle: (f32, f32),
        from: (f32, f32),
        to: (f32, f32),
        nearer: &[(Tilt, (f32, f32))],
    ) -> bool {
        [from, to].into_iter().all(|(x, y)| {
            let (x, y) = self.onto_screen(x, y);
            let (x, y) = (middle.0 + x, middle.1 + y);
            nearer.iter().any(|(card, at)| card.covers(x - at.0, y - at.1, 0.0))
        })
    }

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
        assert!(near(far_x, 111.83) && near(far_y, 86.99), "{far_x} {far_y}");
        assert!(near(close_x, -151.18) && near(close_y, 117.60), "{close_x} {close_y}");
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
    fn a_stretch_is_hidden_only_when_both_its_ends_are_under_a_nearer_card() {
        let neighbour = Tilt::new(400.0, 200.0, -50.0);
        let (edge, _) = neighbour.onto_screen(200.0, 0.0);
        let middle = [(Tilt::new(460.0, 230.0, 0.0), (edge - 40.0 + 230.0, 0.0))];
        let at = |on_screen: f32| neighbour.onto_card(on_screen, 0.0).unwrap().0;
        assert!(neighbour.hidden_under((0.0, 0.0), (at(edge - 30.0), -80.0), (at(edge - 2.0), -80.0), &middle));
        assert!(!neighbour.hidden_under((0.0, 0.0), (at(edge - 60.0), -80.0), (at(edge - 2.0), -80.0), &middle));
        assert!(!neighbour.hidden_under((0.0, 0.0), (at(edge - 30.0), -80.0), (at(edge - 2.0), -80.0), &[]));
    }

    #[test]
    fn the_box_holds_the_tall_side_and_the_slanted_edges_leave_its_corners_out() {
        let turned = Tilt::new(400.0, 200.0, 50.0);
        let [left, top, right, bottom] = turned.reach(0.0);
        assert!(near(left, -151.18) && near(right, 111.83) && near(top, -117.60) && near(bottom, 117.60));
        assert!(!turned.covers(right - 1.0, top + 1.0, 0.0));
        assert!(turned.covers(left + 1.0, top + 1.0, 0.0));
        assert!(turned.covers(0.0, 0.0, 0.0));
    }
}
