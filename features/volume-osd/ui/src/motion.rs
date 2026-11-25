use crate::tokens::{
    FIZZ, FIZZ_PACE, FRAME_MAX, GONE, IDLE_WAVE, LEVEL, SLIDE, SLOSH_DECAY, SLOSH_GAIN, SLOSH_MAX, TILT, TILT_GAIN,
    TILT_MAX,
};
use nova_look::motion::SpringState;
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiquidMotion {
    pub open_target: f32,
    pub level_target: f32,
    pub still: bool,
    pub open: SpringState,
    pub level: SpringState,
    pub tilt: SpringState,
    pub time: f32,
    pub rise: f32,
    pub slosh: f32,
    pub amp: f32,
    pub fizz: f32,
}

impl LiquidMotion {
    pub fn at(level: f32, still: bool) -> Self {
        Self { level_target: level, level: SpringState::at(level), still, ..Self::default() }
    }

    pub fn shown(&self) -> bool {
        self.open.value > GONE || self.open_target > 0.0
    }

    pub fn advance(&mut self, elapsed: Duration) {
        let elapsed = elapsed.min(FRAME_MAX);
        let dt = elapsed.as_secs_f32();
        self.time += dt;
        self.open = SLIDE.step(self.open, self.open_target, elapsed);
        let mut level = LEVEL.step(self.level, self.level_target, elapsed);
        level.value = level.value.clamp(0.0, 1.0);
        self.level = level;
        let mut swing = TILT.step(self.tilt, 0.0, elapsed);
        swing.speed -= self.open.acceleration * TILT_GAIN * dt;
        swing.value = swing.value.clamp(-TILT_MAX, TILT_MAX);
        self.tilt = swing;
        let stirred = self.level.speed.abs() * SLOSH_GAIN * dt;
        self.slosh = SLOSH_MAX.min(self.slosh * (-dt / SLOSH_DECAY).exp() + stirred);
        self.amp = if self.still { 0.0 } else { IDLE_WAVE } + self.slosh;
        self.fizz =
            if self.still { 0.0 } else { (FIZZ + self.slosh / SLOSH_MAX + self.tilt.value.abs() / TILT_MAX).min(1.0) };
        self.rise += dt * (1.0 + FIZZ_PACE * self.fizz);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(8);

    fn run(motion: &mut LiquidMotion, frames: usize) {
        for _ in 0..frames {
            motion.advance(FRAME);
        }
    }

    #[test]
    fn it_comes_out_overshooting_and_goes_back_in() {
        let mut motion = LiquidMotion::at(0.4, false);
        assert!(!motion.shown());
        motion.open_target = 1.0;
        assert!(motion.shown(), "shown from the moment it's asked out");
        let mut highest = 0.0_f32;
        for _ in 0..200 {
            motion.advance(FRAME);
            highest = highest.max(motion.open.value);
        }
        assert!(highest > 1.02, "it never overshot: {highest}");
        let room = crate::logic::widest() / f32::from(crate::tokens::WIDTH + crate::tokens::EDGE_GAP);
        assert!(highest <= room, "its window is too narrow for the overshoot: {highest}");
        assert!((motion.open.value - 1.0).abs() < 1e-3);
        motion.open_target = 0.0;
        run(&mut motion, 400);
        assert!(!motion.shown());
    }

    #[test]
    fn sliding_tilts_the_surface_and_it_swings_back() {
        let mut motion = LiquidMotion::at(0.4, false);
        motion.open_target = 1.0;
        run(&mut motion, 10);
        assert!(motion.tilt.value != 0.0, "the slide tilts it");
        assert!(motion.tilt.value.abs() <= TILT_MAX);
        run(&mut motion, 1000);
        assert!(motion.tilt.value.abs() < 0.01, "{}", motion.tilt.value);
    }

    #[test]
    fn a_level_change_sloshes_and_fizzes_then_calms() {
        let mut motion = LiquidMotion::at(0.2, false);
        motion.open_target = 1.0;
        run(&mut motion, 300);
        let calm = motion.fizz;
        assert!((calm - FIZZ).abs() < 0.02, "{calm}");
        motion.level_target = 0.9;
        run(&mut motion, 20);
        assert!(motion.slosh > 0.5 && motion.fizz > calm, "{} {}", motion.slosh, motion.fizz);
        assert!(motion.slosh <= SLOSH_MAX);
        run(&mut motion, 600);
        assert!((motion.level.value - 0.9).abs() < 1e-3);
        assert!(motion.slosh < 0.05, "{}", motion.slosh);
    }

    #[test]
    fn a_still_surface_has_no_waves_or_specks() {
        let mut motion = LiquidMotion::at(0.4, true);
        motion.open_target = 1.0;
        run(&mut motion, 300);
        assert!(motion.amp < 0.05 && motion.fizz == 0.0, "{} {}", motion.amp, motion.fizz);
    }

    #[test]
    fn the_level_stays_between_empty_and_full() {
        let mut motion = LiquidMotion::at(0.98, false);
        motion.level_target = 1.0;
        motion.level.speed = 50.0;
        motion.advance(FRAME);
        assert!(motion.level.value <= 1.0);
    }

    #[test]
    fn a_stalled_frame_moves_it_one_frame_s_worth() {
        let mut motion = LiquidMotion::at(0.4, false);
        motion.open_target = 1.0;
        motion.advance(Duration::from_secs(3));
        assert!((motion.time - FRAME_MAX.as_secs_f32()).abs() < 1e-6);
    }
}
