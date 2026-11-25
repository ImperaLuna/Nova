use std::time::{Duration, Instant};

const REST_DISTANCE: f32 = 0.05;
const FRAME_MAX: Duration = Duration::from_millis(100);
const CORNER_POINTS: usize = 4;

#[derive(Debug, Clone, Default)]
pub struct Levels {
    heights: Vec<f32>,
    eased_at: Option<Instant>,
}

impl Levels {
    pub fn ease(&mut self, targets: &[f32], rise: Duration, fall: Duration, now: Instant) -> &[f32] {
        if self.heights.len() != targets.len() {
            self.heights = targets.to_vec();
        }
        let elapsed = self.eased_at.map_or(Duration::ZERO, |at| now.saturating_duration_since(at).min(FRAME_MAX));
        for (height, target) in self.heights.iter_mut().zip(targets) {
            *height = approach(*height, *target, if target > height { rise } else { fall }, elapsed);
        }
        self.eased_at = (!self.settled(targets)).then_some(now);
        &self.heights
    }

    pub fn heights(&self) -> &[f32] {
        &self.heights
    }

    pub fn settled(&self, targets: &[f32]) -> bool {
        self.heights.len() == targets.len() && self.heights.iter().zip(targets).all(|(height, target)| height == target)
    }
}

fn approach(from: f32, to: f32, tau: Duration, elapsed: Duration) -> f32 {
    let left = if tau.is_zero() { 0.0 } else { (-elapsed.as_secs_f32() / tau.as_secs_f32()).exp() };
    let value = to + (from - to) * left;
    if (value - to).abs() < REST_DISTANCE { to } else { value }
}

pub fn outline(heights: &[f32], width: f32, gap: f32, top: f32, samples: usize) -> Vec<(f32, f32)> {
    let (Some(first), Some(last)) = (heights.first(), heights.last()) else { return Vec::new() };
    let radius = width / 2.0;
    let total = width * heights.len() as f32 + gap * heights.len().saturating_sub(1) as f32;
    let height = |value: f32| value.max(width).min(top);
    let middles: Vec<(f32, f32)> = heights
        .iter()
        .enumerate()
        .map(|(index, value)| (radius + (width + gap) * index as f32, height(*value)))
        .collect();

    let mut points = Vec::new();
    corner(&mut points, (radius, radius), radius, 270.0);
    corner(&mut points, (radius, height(*first) - radius), radius, 180.0);
    for (index, pair) in middles.windows(2).enumerate() {
        let before = middles[index.saturating_sub(1)];
        let after = middles[(index + 2).min(middles.len() - 1)];
        for step in 1..=samples.max(1) {
            let along = step as f32 / samples.max(1) as f32;
            let (x, y) = catmull_rom(before, pair[0], pair[1], after, along);
            points.push((x, y.max(width).min(top)));
        }
    }
    corner(&mut points, (total - radius, height(*last) - radius), radius, 90.0);
    corner(&mut points, (total - radius, radius), radius, 0.0);
    points
}

fn corner(points: &mut Vec<(f32, f32)>, center: (f32, f32), radius: f32, start: f32) {
    for step in 0..=CORNER_POINTS {
        let angle = (start - 90.0 * step as f32 / CORNER_POINTS as f32).to_radians();
        points.push((center.0 + radius * angle.cos(), center.1 + radius * angle.sin()));
    }
}

fn catmull_rom(before: (f32, f32), from: (f32, f32), to: (f32, f32), after: (f32, f32), along: f32) -> (f32, f32) {
    let (t, t2, t3) = (along, along * along, along * along * along);
    let at = |p0: f32, p1: f32, p2: f32, p3: f32| {
        0.5 * (2.0 * p1
            + (p2 - p0) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
            + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3)
    };
    (at(before.0, from.0, to.0, after.0), at(before.1, from.1, to.1, after.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RISE: Duration = Duration::from_millis(40);
    const FALL: Duration = Duration::from_millis(160);
    const FRAME: Duration = Duration::from_millis(8);

    fn run(levels: &mut Levels, targets: &[f32], start: Instant, frames: u32) -> Vec<f32> {
        let mut heights = Vec::new();
        for frame in 0..=frames {
            heights = levels.ease(targets, RISE, FALL, start + FRAME * frame).to_vec();
        }
        heights
    }

    #[test]
    fn the_first_frame_starts_at_the_targets() {
        let mut levels = Levels::default();
        assert_eq!(levels.ease(&[3.0, 12.0], RISE, FALL, Instant::now()), [3.0, 12.0]);
        assert!(levels.settled(&[3.0, 12.0]));
    }

    #[test]
    fn bars_rise_quicker_than_they_fall() {
        let start = Instant::now();
        let mut levels = Levels::default();
        levels.ease(&[3.0, 12.0], RISE, FALL, start);
        let heights = run(&mut levels, &[12.0, 3.0], start, 5);
        assert!(heights[0] > 3.0 + 9.0 * 0.6, "{heights:?}");
        assert!(heights[1] > 12.0 - 9.0 * 0.3, "{heights:?}");
        assert!(!levels.settled(&[12.0, 3.0]));
    }

    #[test]
    fn they_arrive_and_then_need_no_frames() {
        let start = Instant::now();
        let mut levels = Levels::default();
        levels.ease(&[12.0; 4], RISE, FALL, start);
        assert_eq!(run(&mut levels, &[3.0; 4], start, 200), [3.0; 4]);
        assert!(levels.settled(&[3.0; 4]));
    }

    #[test]
    fn a_stall_does_not_make_them_jump() {
        let start = Instant::now();
        let mut levels = Levels::default();
        levels.ease(&[3.0], RISE, FALL, start);
        levels.ease(&[12.0], RISE, FALL, start + FRAME);
        let after_stall = levels.ease(&[12.0], RISE, FALL, start + FRAME + Duration::from_secs(5))[0];
        let one_long_frame = approach(3.0, 12.0, RISE, FRAME_MAX);
        assert!((after_stall - one_long_frame).abs() < 0.01, "{after_stall} vs {one_long_frame}");
    }

    #[test]
    fn the_outline_passes_through_each_bar_and_stays_in_the_box() {
        let heights = [3.0, 12.0, 6.0, 9.0];
        let points = outline(&heights, 3.0, 2.0, 12.0, 6);
        for (index, height) in heights.iter().enumerate() {
            let middle = 1.5 + 5.0 * index as f32;
            assert!(
                points.iter().any(|(x, y)| (x - middle).abs() < 1e-4 && (y - height).abs() < 1e-4),
                "no point at bar {index}'s top"
            );
        }
        assert!(points.iter().all(|(x, y)| (-1e-4..=18.0 + 1e-4).contains(x) && (-1e-4..=12.0 + 1e-4).contains(y)));
    }

    #[test]
    fn the_outline_stands_on_the_foot_with_round_ends() {
        let points = outline(&[3.0, 3.0], 3.0, 2.0, 12.0, 4);
        let (first, last) = (points[0], points[points.len() - 1]);
        assert!((first.0 - 1.5).abs() < 1e-4 && first.1.abs() < 1e-4, "starts at the foot's left: {first:?}");
        assert!((last.0 - 6.5).abs() < 1e-4 && last.1.abs() < 1e-4, "ends at the foot's right: {last:?}");
        assert!(points.iter().any(|(x, y)| x.abs() < 1e-4 && (y - 1.5).abs() < 1e-4), "left end is round");
        assert!(outline(&[], 3.0, 2.0, 12.0, 4).is_empty());
    }
}
