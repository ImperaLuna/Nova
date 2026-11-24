//! The bar stepping aside for a fullscreen window, on one screen: while the
//! workspace it shows has a window over the whole screen, the pills slide up
//! out of sight, then the bar's room is given back, so the windows beside the
//! fullscreen one get the full height. When that ends the room comes back
//! first, then the pills, so they never land on a window. Meanwhile the pointer
//! touching the top edge brings the pills back over the window (they stay a
//! moment after it leaves), and they stay while a panel is open on the screen.
//!
//! Plain state over time, tested alone: the stage tells it what happened
//! and when, asks it where the pills are, and wakes it at
//! [`Aside::next_wake`].

use nova_look::motion::Eased;
use nova_look::tokens::{DURATION_PANEL, DURATION_PEEK_LINGER};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub struct Aside {
    /// The shown workspace has a fullscreen window, and the bar has pills.
    fullscreen: bool,
    /// A panel is open on the screen.
    held: bool,
    /// The pointer is on the strip along the top edge or on a pill.
    hovered: bool,
    /// The pills stay until then after the pointer left.
    linger_until: Option<Instant>,
    /// The bar keeps its room at the top of the screen.
    room_kept: bool,
    /// The room goes then, once the pills have slid away.
    room_goes_at: Option<Instant>,
    /// Tucked pills wait for the room to come back until then.
    waiting_until: Option<Instant>,
    /// 0 in place, 1 out of sight above the screen.
    slide: Eased,
}

impl Aside {
    pub fn new(now: Instant) -> Self {
        Self {
            fullscreen: false,
            held: false,
            hovered: false,
            linger_until: None,
            room_kept: true,
            room_goes_at: None,
            waiting_until: None,
            slide: Eased::at(0.0, now),
        }
    }

    fn lingering(&self, now: Instant) -> bool {
        self.linger_until.is_some_and(|until| now < until)
    }

    fn waiting(&self, now: Instant) -> bool {
        self.waiting_until.is_some_and(|until| now < until)
    }

    /// Brought back over the fullscreen window by the pointer.
    fn peeking(&self, now: Instant) -> bool {
        self.fullscreen && (self.hovered || self.lingering(now))
    }

    /// Out of sight, or on the way there.
    pub fn tucked(&self, now: Instant) -> bool {
        (self.fullscreen || self.waiting(now)) && !self.peeking(now) && !self.held
    }

    /// Whether the shown workspace has a fullscreen window: the strip along
    /// the top edge takes the pointer meanwhile.
    pub fn fullscreen(&self) -> bool {
        self.fullscreen
    }

    pub fn set_fullscreen(&mut self, fullscreen: bool, now: Instant) {
        if fullscreen == self.fullscreen {
            return;
        }
        self.advance(now);
        self.fullscreen = fullscreen;
        if fullscreen {
            self.waiting_until = None;
            self.room_goes_at = Some(now + DURATION_PANEL);
        } else {
            self.room_goes_at = None;
            // Only pills that were out of sight wait; shown ones stay put.
            let waits = !self.held && !self.hovered && !self.lingering(now);
            self.waiting_until = waits.then_some(now + DURATION_PANEL);
            self.room_kept = true;
        }
        self.settle(now);
    }

    /// Whether a panel is open on the screen.
    pub fn held(&self) -> bool {
        self.held
    }

    pub fn set_held(&mut self, held: bool, now: Instant) {
        self.advance(now);
        self.held = held;
        self.settle(now);
    }

    pub fn hovered(&self) -> bool {
        self.hovered
    }

    pub fn set_hovered(&mut self, hovered: bool, now: Instant) {
        if hovered == self.hovered {
            return;
        }
        self.advance(now);
        self.hovered = hovered;
        // Only over a fullscreen window: elsewhere it would wake the stage
        // for nothing.
        self.linger_until = (!hovered && self.fullscreen).then_some(now + DURATION_PEEK_LINGER);
        self.settle(now);
    }

    /// Runs what was due by `now`. Returns whether the room changed, for
    /// the stage to tell the compositor.
    pub fn advance(&mut self, now: Instant) -> bool {
        let before = self.room_kept;
        if self.room_goes_at.is_some_and(|at| now >= at) {
            self.room_goes_at = None;
            self.room_kept = false;
        }
        if !self.waiting(now) {
            self.waiting_until = None;
        }
        if !self.lingering(now) {
            self.linger_until = None;
        }
        self.settle(now);
        before != self.room_kept
    }

    fn settle(&mut self, now: Instant) {
        self.slide.set_target(if self.tucked(now) { 1.0 } else { 0.0 }, now);
    }

    /// When something is due next: the stage wakes it then.
    pub fn next_wake(&self) -> Option<Instant> {
        [self.room_goes_at, self.waiting_until, self.linger_until].into_iter().flatten().min()
    }

    pub fn room_kept(&self) -> bool {
        self.room_kept
    }

    /// How far the pills have slid out of sight (0..1).
    pub fn slid(&self, now: Instant) -> f32 {
        self.slide.value(now)
    }

    /// Out of sight and at rest: nothing of the bar is drawn.
    pub fn gone(&self, now: Instant) -> bool {
        self.slide.target() == 1.0 && self.slide.settled(now)
    }

    pub fn moving(&self, now: Instant) -> bool {
        !self.slide.settled(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(start: Instant, aside: &mut Aside, by: std::time::Duration) -> Instant {
        let now = start + by;
        aside.advance(now);
        now
    }

    #[test]
    fn the_pills_slide_away_then_the_room_goes() {
        let start = Instant::now();
        let mut aside = Aside::new(start);
        aside.set_fullscreen(true, start);
        assert!(aside.tucked(start));
        assert!(aside.room_kept(), "the windows wait for the pills");
        assert_eq!(aside.next_wake(), Some(start + DURATION_PANEL));
        let slid = start + DURATION_PANEL;
        assert!(aside.advance(slid));
        assert!(!aside.room_kept());
        assert!(aside.gone(slid));
        assert_eq!(aside.next_wake(), None);
    }

    #[test]
    fn the_room_comes_back_first_then_the_pills() {
        let start = Instant::now();
        let mut aside = Aside::new(start);
        aside.set_fullscreen(true, start);
        let gone = after(start, &mut aside, DURATION_PANEL);
        aside.set_fullscreen(false, gone);
        assert!(aside.room_kept());
        assert!(aside.tucked(gone), "still out of sight while the windows move down");
        let back = gone + DURATION_PANEL;
        aside.advance(back);
        assert!(!aside.tucked(back));
        assert_eq!(aside.slid(back), 1.0, "and only now slides in");
        assert_eq!(aside.slid(back + DURATION_PANEL), 0.0);
    }

    #[test]
    fn touching_the_top_edge_brings_the_pills_back_for_a_moment() {
        let start = Instant::now();
        let mut aside = Aside::new(start);
        aside.set_fullscreen(true, start);
        let gone = after(start, &mut aside, DURATION_PANEL);
        aside.set_hovered(true, gone);
        assert!(!aside.tucked(gone));
        assert!(!aside.room_kept(), "over the window, not pushing it");
        let left = gone + DURATION_PANEL;
        aside.set_hovered(false, left);
        assert!(!aside.tucked(left));
        assert_eq!(aside.next_wake(), Some(left + DURATION_PEEK_LINGER));
        let lingered = after(left, &mut aside, DURATION_PEEK_LINGER);
        assert!(aside.tucked(lingered));
    }

    #[test]
    fn an_open_panel_holds_the_pills() {
        let start = Instant::now();
        let mut aside = Aside::new(start);
        aside.set_held(true, start);
        aside.set_fullscreen(true, start);
        assert!(!aside.tucked(start));
        let later = after(start, &mut aside, DURATION_PANEL);
        assert!(!aside.room_kept(), "the room goes all the same");
        aside.set_held(false, later);
        assert!(aside.tucked(later));
    }

    #[test]
    fn pills_that_show_when_the_window_leaves_fullscreen_stay_put() {
        let start = Instant::now();
        let mut aside = Aside::new(start);
        aside.set_fullscreen(true, start);
        let gone = after(start, &mut aside, DURATION_PANEL);
        aside.set_hovered(true, gone);
        let shown = gone + DURATION_PANEL;
        aside.set_fullscreen(false, shown);
        assert!(!aside.tucked(shown));
        assert_eq!(aside.slid(shown + DURATION_PANEL), 0.0);
    }

    #[test]
    fn leaving_fullscreen_before_the_room_went_keeps_it() {
        let start = Instant::now();
        let mut aside = Aside::new(start);
        aside.set_fullscreen(true, start);
        let soon = start + DURATION_PANEL / 2;
        aside.set_fullscreen(false, soon);
        assert!(!aside.advance(soon + DURATION_PANEL));
        assert!(aside.room_kept());
    }
}
