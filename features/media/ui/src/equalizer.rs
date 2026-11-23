//! The pill's equalizer between its steps, with no drawing in it. The
//! model picks new heights every `DURATION_EQ_STEP`; [`Levels`] eases the
//! drawn heights toward them each frame, quick on the way up and slower on
//! the way down, and [`outline`] joins them into one smooth shape.
//!
//! Today's shell (`ui/media/Equalizer.qml`) jumps straight to each step.

use std::time::{Duration, Instant};

/// Closer to its target than this (px), a bar is there: less than a
/// twentieth of a pixel, too little to see.
const REST_DISTANCE: f32 = 0.05;
/// The longest time one frame may cover: after a stall the bars carry on
/// from where they were instead of jumping.
const FRAME_MAX: Duration = Duration::from_millis(100);
/// Points on each quarter circle that rounds a corner of the outline.
const CORNER_POINTS: usize = 4;

/// The heights drawn now, easing toward the model's.
#[derive(Debug, Clone, Default)]
pub struct Levels {
    heights: Vec<f32>,
    /// When the last frame eased them, while they move.
    eased_at: Option<Instant>,
}

impl Levels {
    /// The heights at `now`, eased toward `targets` since the last frame:
    /// each closes in on its target with time constant `rise` going up and
    /// `fall` going down. The first call, or a change in the number of
    /// bars, starts at the targets.
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

    /// The heights the last [`Levels::ease`] gave.
    pub fn heights(&self) -> &[f32] {
        &self.heights
    }

    /// Whether every bar is at its target, so no more frames are needed.
    pub fn settled(&self, targets: &[f32]) -> bool {
        self.heights.len() == targets.len() && self.heights.iter().zip(targets).all(|(height, target)| height == target)
    }
}

/// `from` after `elapsed` on its way to `to`, closing in with time
/// constant `tau`; at `to` once within [`REST_DISTANCE`].
fn approach(from: f32, to: f32, tau: Duration, elapsed: Duration) -> f32 {
    let left = if tau.is_zero() { 0.0 } else { (-elapsed.as_secs_f32() / tau.as_secs_f32()).exp() };
    let value = to + (from - to) * left;
    if (value - to).abs() < REST_DISTANCE { to } else { value }
}

/// One shape for bars of `heights`, each `width` wide and `gap` apart: a
/// smooth top through the middle of each bar's top, its ends and its foot
/// rounded like a bar's, `samples` points between two bars. Points go
/// round it from the bottom left, `x` from the left edge and `y` up from
/// the foot, all within the bars' box (`top` high).
pub fn outline(heights: &[f32], width: f32, gap: f32, top: f32, samples: usize) -> Vec<(f32, f32)> {
    let (Some(first), Some(last)) = (heights.first(), heights.last()) else { return Vec::new() };
    let radius = width / 2.0;
    let total = width * heights.len() as f32 + gap * heights.len().saturating_sub(1) as f32;
    let height = |value: f32| value.clamp(width, top);
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
            points.push((x, y.clamp(width, top)));
        }
    }
    corner(&mut points, (total - radius, height(*last) - radius), radius, 90.0);
    corner(&mut points, (total - radius, radius), radius, 0.0);
    points
}

/// A quarter circle of `radius` round `center`, from `start` degrees on
/// clockwise (`y` up): the outline turns right at each corner.
fn corner(points: &mut Vec<(f32, f32)>, center: (f32, f32), radius: f32, start: f32) {
    for step in 0..=CORNER_POINTS {
        let angle = (start - 90.0 * step as f32 / CORNER_POINTS as f32).to_radians();
        points.push((center.0 + radius * angle.cos(), center.1 + radius * angle.sin()));
    }
}

/// The point `along` (0..1) the way from `from` to `to` on a Catmull-Rom
/// curve, which passes through every point it is given.
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

    /// Eases `levels` toward `targets` for `frames` frames from `start`.
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
        // 40 ms later: the rising bar is most of the way, the falling one
        // not yet a third.
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
