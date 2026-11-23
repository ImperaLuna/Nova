use crate::tokens::{DURATION_PANEL, EASING};
use std::time::{Duration, Instant};

const SOLVE_ROUNDS: usize = 8;
const SOLVE_FLAT_SLOPE: f32 = 1e-5;

fn bezier(at: f32, first: f32, second: f32) -> f32 {
    let rest = 1.0 - at;
    3.0 * rest * rest * at * first + 3.0 * rest * at * at * second + at * at * at
}

fn bezier_slope(at: f32, first: f32, second: f32) -> f32 {
    let rest = 1.0 - at;
    3.0 * rest * rest * first + 6.0 * rest * at * (second - first) + 3.0 * at * at * (1.0 - second)
}

pub fn cubic_bezier(progress: f32, [x1, y1, x2, y2]: [f32; 4]) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    let mut at = progress;
    for _ in 0..SOLVE_ROUNDS {
        let slope = bezier_slope(at, x1, x2);
        if slope.abs() < SOLVE_FLAT_SLOPE {
            break;
        }
        at = (at - (bezier(at, x1, x2) - progress) / slope).clamp(0.0, 1.0);
    }
    bezier(at, y1, y2)
}

pub fn ease(progress: f32) -> f32 {
    cubic_bezier(progress, EASING)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Eased {
    from: f32,
    to: f32,
    started: Instant,
    duration: Duration,
}

impl Eased {
    pub fn at(value: f32, now: Instant) -> Self {
        Self::lasting(value, DURATION_PANEL, now)
    }

    pub fn lasting(value: f32, duration: Duration, now: Instant) -> Self {
        let started = now.checked_sub(duration).unwrap_or(now);
        Self { from: value, to: value, started, duration }
    }

    pub fn set_target(&mut self, target: f32, now: Instant) {
        if target == self.to {
            return;
        }
        self.from = self.value(now);
        self.to = target;
        self.started = now;
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn value(&self, now: Instant) -> f32 {
        self.from + (self.to - self.from) * ease(self.progress(now))
    }

    pub fn settled(&self, now: Instant) -> bool {
        self.from == self.to || self.progress(now) >= 1.0
    }

    fn progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        (now.saturating_duration_since(self.started).as_secs_f32() / self.duration.as_secs_f32()).min(1.0)
    }
}

const SPRING_STEP: f32 = 1.0 / 240.0;
const SPRING_FRAME_MAX: f32 = 1.0 / 30.0;
const SPRING_REST_DISTANCE: f32 = 0.0005;
const SPRING_REST_SPEED: f32 = 0.005;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    pub stiffness: f32,
    pub damping: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpringState {
    pub value: f32,
    pub speed: f32,
    pub acceleration: f32,
}

impl SpringState {
    pub fn at(value: f32) -> Self {
        Self { value, ..Self::default() }
    }
}

impl Spring {
    pub fn step(self, mut state: SpringState, target: f32, elapsed: Duration) -> SpringState {
        let friction = 2.0 * self.stiffness.sqrt() * self.damping;
        let mut left = elapsed.as_secs_f32().min(SPRING_FRAME_MAX);
        while left > 0.0 {
            let step = left.min(SPRING_STEP);
            state.acceleration = -self.stiffness * (state.value - target) - friction * state.speed;
            state.speed += state.acceleration * step;
            state.value += state.speed * step;
            left -= step;
        }
        if self.settled(state, target) {
            return SpringState::at(target);
        }
        state
    }

    pub fn settled(self, state: SpringState, target: f32) -> bool {
        (state.value - target).abs() < SPRING_REST_DISTANCE && state.speed.abs() < SPRING_REST_SPEED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(8);

    #[test]
    fn the_panel_curve_starts_fast_and_stops_gently() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert!(ease(0.33) > 0.8, "{}", ease(0.33));
        let mut previous = 0.0;
        for step in 1..=100 {
            let now = ease(step as f32 / 100.0);
            assert!(now >= previous, "the curve went back at {step}%");
            previous = now;
        }
    }

    #[test]
    fn a_linear_bezier_is_the_identity() {
        for step in 0..=10 {
            let progress = step as f32 / 10.0;
            let eased = cubic_bezier(progress, [0.25, 0.25, 0.75, 0.75]);
            assert!((eased - progress).abs() < 1e-4, "{progress} gave {eased}");
        }
    }

    #[test]
    fn an_eased_value_at_rest_needs_no_frames() {
        let start = Instant::now();
        let value = Eased::at(1.0, start);
        assert!(value.settled(start));
        assert_eq!(value.value(start), 1.0);
    }

    #[test]
    fn an_eased_value_turned_round_half_way_continues_from_where_it_is() {
        let start = Instant::now();
        let mut value = Eased::at(0.0, start);
        value.set_target(1.0, start);
        assert!(!value.settled(start));
        let half_way = start + DURATION_PANEL / 2;
        let reached = value.value(half_way);
        assert!(reached > 0.5 && reached < 1.0, "{reached}");

        value.set_target(0.0, half_way);
        assert_eq!(value.value(half_way), reached);
        assert!(!value.settled(half_way + DURATION_PANEL / 2));
        assert!(value.settled(half_way + DURATION_PANEL));
        assert_eq!(value.value(half_way + DURATION_PANEL), 0.0);
    }

    #[test]
    fn setting_the_same_target_again_does_not_restart_the_motion() {
        let start = Instant::now();
        let mut value = Eased::at(0.0, start);
        value.set_target(1.0, start);
        let later = start + DURATION_PANEL / 2;
        let before = value.value(later);
        value.set_target(1.0, later);
        assert_eq!(value.value(later), before);
        assert!(value.settled(start + DURATION_PANEL));
    }

    fn run(spring: Spring) -> (f32, usize) {
        let mut state = SpringState::at(0.0);
        let mut highest = 0.0_f32;
        for frame in 1..=2000 {
            state = spring.step(state, 1.0, FRAME);
            highest = highest.max(state.value);
            if spring.settled(state, 1.0) {
                assert_eq!(state, SpringState::at(1.0));
                return (highest, frame);
            }
        }
        (highest, usize::MAX)
    }

    #[test]
    fn an_underdamped_spring_overshoots_then_rests_on_its_target() {
        let (highest, frames) = run(Spring { stiffness: 170.0, damping: 0.62 });
        assert!(highest > 1.02, "it never overshot: {highest}");
        assert!(frames < 250, "it took {frames} frames to rest");
    }

    #[test]
    fn a_critically_damped_spring_arrives_without_overshoot() {
        let (highest, frames) = run(Spring { stiffness: 170.0, damping: 1.0 });
        assert!(highest <= 1.0, "{highest}");
        assert!(frames < 250, "it took {frames} frames to rest");
    }

    #[test]
    fn a_stalled_frame_does_not_throw_a_stiff_spring() {
        let spring = Spring { stiffness: 900.0, damping: 0.3 };
        let state = spring.step(SpringState::at(0.0), 1.0, Duration::from_secs(5));
        assert!(state.value.is_finite() && state.value.abs() < 2.0, "{state:?}");
    }
}
