//! When a panel's content shows. Opening, it waits until the panel has
//! grown part of the way, then fades in; closing, it follows the morph
//! back, all at once. `MorphReveal` in today's shell.

use nova_look::tokens::{DURATION_FADE, DURATION_PANEL, REVEAL_CONTENT};
use std::time::Instant;

/// The opacity of a panel's content, from when it last turned round.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reveal {
    opening: bool,
    turned: Instant,
    /// Its opacity, and how far the panel had grown, when it turned round.
    opacity_then: f32,
    morph_then: f32,
}

impl Reveal {
    /// Hidden, in a closed panel.
    pub fn hidden(now: Instant) -> Self {
        Self { opening: false, turned: now, opacity_then: 0.0, morph_then: 0.0 }
    }

    /// Heads for shown or hidden from where it is. `morph` is how far the
    /// panel has grown (0..1) at `now`.
    pub fn set_open(&mut self, open: bool, morph: f32, now: Instant) {
        if open == self.opening {
            return;
        }
        self.opacity_then = self.opacity(morph, now);
        self.morph_then = morph;
        self.opening = open;
        self.turned = now;
    }

    pub fn opacity(&self, morph: f32, now: Instant) -> f32 {
        if !self.opening {
            // Down with the panel: gone when it is closed.
            let left = if self.morph_then > 0.0 { (morph / self.morph_then).clamp(0.0, 1.0) } else { 0.0 };
            return self.opacity_then * left;
        }
        let waited = now.saturating_duration_since(self.turned).as_secs_f32();
        let delay = DURATION_PANEL.as_secs_f32() * REVEAL_CONTENT;
        let faded = ((waited - delay) / DURATION_FADE.as_secs_f32()).clamp(0.0, 1.0);
        self.opacity_then + (1.0 - self.opacity_then) * faded
    }

    /// Whether it needs no more frames of its own: closing, it only
    /// follows the morph.
    pub fn settled(&self, now: Instant) -> bool {
        !self.opening
            || now.saturating_duration_since(self.turned) >= DURATION_PANEL.mul_f32(REVEAL_CONTENT) + DURATION_FADE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn content_waits_for_the_panel_then_fades_in() {
        let start = Instant::now();
        let mut reveal = Reveal::hidden(start);
        assert!(reveal.settled(start));
        reveal.set_open(true, 0.0, start);
        let delay = DURATION_PANEL.mul_f32(REVEAL_CONTENT);
        assert_eq!(reveal.opacity(0.9, start + delay), 0.0);
        let half = reveal.opacity(1.0, start + delay + DURATION_FADE / 2);
        assert!((half - 0.5).abs() < 0.01, "{half}");
        assert!(!reveal.settled(start + delay));
        let shown = start + delay + DURATION_FADE;
        assert!(reveal.opacity(1.0, shown) > 0.999);
        assert!(reveal.settled(shown));
    }

    #[test]
    fn content_goes_with_the_panel_as_it_closes() {
        let start = Instant::now();
        let mut reveal = Reveal::hidden(start);
        reveal.set_open(true, 0.0, start);
        let later = start + Duration::from_secs(1);
        reveal.set_open(false, 1.0, later);
        assert_eq!(reveal.opacity(1.0, later), 1.0);
        assert_eq!(reveal.opacity(0.25, later), 0.25);
        assert_eq!(reveal.opacity(0.0, later), 0.0);
    }

    #[test]
    fn turning_round_before_the_content_showed_keeps_it_hidden() {
        let start = Instant::now();
        let mut reveal = Reveal::hidden(start);
        reveal.set_open(true, 0.0, start);
        let early = start + Duration::from_millis(20);
        reveal.set_open(false, 0.4, early);
        assert_eq!(reveal.opacity(0.4, early), 0.0);
        assert_eq!(reveal.opacity(0.1, early), 0.0);
    }
}
