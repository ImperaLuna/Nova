use nova_look::motion::Eased;
use nova_look::tokens::{DURATION_PANEL, DURATION_PEEK_LINGER};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub struct Aside {
    fullscreen: bool,
    held: bool,
    hovered: bool,
    linger_until: Option<Instant>,
    room_kept: bool,
    room_goes_at: Option<Instant>,
    waiting_until: Option<Instant>,
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

    fn peeking(&self, now: Instant) -> bool {
        self.fullscreen && (self.hovered || self.lingering(now))
    }

    pub fn tucked(&self, now: Instant) -> bool {
        (self.fullscreen || self.waiting(now)) && !self.peeking(now) && !self.held
    }

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
            let waits = !self.held && !self.hovered && !self.lingering(now);
            self.waiting_until = waits.then_some(now + DURATION_PANEL);
            self.room_kept = true;
        }
        self.settle(now);
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
        self.linger_until = (!hovered && self.fullscreen).then_some(now + DURATION_PEEK_LINGER);
        self.settle(now);
    }

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

    pub fn next_wake(&self) -> Option<Instant> {
        [self.room_goes_at, self.waiting_until, self.linger_until].into_iter().flatten().min()
    }

    pub fn room_kept(&self) -> bool {
        self.room_kept
    }

    pub fn slid(&self, now: Instant) -> f32 {
        self.slide.value(now)
    }

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
