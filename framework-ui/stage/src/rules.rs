//! Which panels are open, on every screen, and what opening one closes: the
//! stage contract the user decided, kept apart from any drawing so it can
//! be tested alone.
//!
//! - A panel is open on one screen at a time: opening it closes it on the
//!   others.
//! - Opening an `exclusive` panel closes every other exclusive one, on any
//!   screen.
//! - Esc, or a click outside, closes every open panel that closes on it,
//!   on every screen.
//! - While a panel that takes typing or closes on Esc is open, its screen's
//!   stage holds the keyboard, so typing doesn't reach the window below.

use gpui::{DisplayId, SharedString};

/// What the rules need to know of a panel (`crate::Panel` has the rest).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelRules {
    /// Its feature's id, which is what `nova toggle` calls it.
    pub name: SharedString,
    pub exclusive: bool,
    pub wants_keyboard: bool,
    pub closes_on_click_outside: bool,
}

impl PanelRules {
    /// Whether Esc closes it: one that takes typing, and every one that
    /// closes on a click outside.
    pub fn closes_on_esc(&self) -> bool {
        self.wants_keyboard || self.closes_on_click_outside
    }

    /// Whether `other` opening closes this one.
    fn ruled_out_by(&self, other: &PanelRules) -> bool {
        other.name == self.name || (other.exclusive && self.exclusive)
    }
}

/// The panels open right now, each with the screen it is open on.
#[derive(Debug, Default)]
pub struct OpenPanels {
    open: Vec<(DisplayId, PanelRules)>,
}

impl OpenPanels {
    pub fn is_open(&self, name: &str, screen: DisplayId) -> bool {
        self.open.iter().any(|(on, panel)| *on == screen && panel.name.as_ref() == name)
    }

    /// Opens `panel` on `screen`, closing what it rules out. Returns
    /// whether anything changed.
    pub fn open(&mut self, panel: PanelRules, screen: DisplayId) -> bool {
        if self.is_open(&panel.name, screen) {
            return false;
        }
        self.open.retain(|(_, other)| !other.ruled_out_by(&panel));
        self.open.push((screen, panel));
        true
    }

    pub fn close(&mut self, name: &str, screen: DisplayId) -> bool {
        self.retain(|on, panel| !(on == screen && panel.name.as_ref() == name))
    }

    pub fn toggle(&mut self, panel: PanelRules, screen: DisplayId) -> bool {
        if self.is_open(&panel.name, screen) { self.close(&panel.name, screen) } else { self.open(panel, screen) }
    }

    /// Esc, or a click outside: closes the panels that close on it.
    pub fn dismiss(&mut self) -> bool {
        self.retain(|_, panel| !panel.closes_on_esc())
    }

    /// Keeps the open panels `keep` says still exist: a screen that left,
    /// or a feature turned off, takes its open panels with it.
    pub fn retain(&mut self, mut keep: impl FnMut(DisplayId, &PanelRules) -> bool) -> bool {
        let before = self.open.len();
        self.open.retain(|(screen, panel)| keep(*screen, panel));
        self.open.len() != before
    }

    /// Whether `screen`'s stage holds the keyboard.
    pub fn needs_keyboard(&self, screen: DisplayId) -> bool {
        self.open.iter().any(|(on, panel)| *on == screen && panel.closes_on_esc())
    }

    /// Whether any screen has a panel open, so everything Nova draws on
    /// `screen` sits over a fullscreen window too.
    pub fn any_open(&self, screen: DisplayId) -> bool {
        self.open.iter().any(|(on, _)| *on == screen)
    }

    /// Whether a panel that closes on a click outside is open on any
    /// screen, so every screen takes clicks everywhere.
    pub fn catches_clicks(&self) -> bool {
        self.open.iter().any(|(_, panel)| panel.closes_on_click_outside)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn left() -> DisplayId {
        DisplayId::new(1)
    }

    fn right() -> DisplayId {
        DisplayId::new(2)
    }

    fn panel(name: &'static str) -> PanelRules {
        PanelRules { name: name.into(), exclusive: true, wants_keyboard: false, closes_on_click_outside: true }
    }

    /// One that stays open beside the others, like a notes window would.
    fn notes() -> PanelRules {
        PanelRules { exclusive: false, closes_on_click_outside: false, ..panel("notes") }
    }

    #[test]
    fn opening_a_panel_closes_the_other_exclusive_ones_on_every_screen() {
        let mut open = OpenPanels::default();
        assert!(open.open(panel("launcher"), left()));
        assert!(open.open(panel("power"), right()));
        assert!(!open.is_open("launcher", left()));
        assert!(open.is_open("power", right()));
    }

    #[test]
    fn a_panel_is_open_on_one_screen_at_a_time() {
        let mut open = OpenPanels::default();
        open.open(notes(), left());
        open.open(notes(), right());
        assert!(!open.is_open("notes", left()));
        assert!(open.is_open("notes", right()));
    }

    #[test]
    fn a_panel_that_is_not_exclusive_stays_open_beside_one_that_is() {
        let mut open = OpenPanels::default();
        open.open(notes(), left());
        open.open(panel("launcher"), left());
        assert!(open.is_open("notes", left()) && open.is_open("launcher", left()));
        open.open(panel("power"), left());
        assert!(open.is_open("notes", left()) && !open.is_open("launcher", left()));
    }

    #[test]
    fn toggling_opens_then_closes_on_that_screen_only() {
        let mut open = OpenPanels::default();
        assert!(open.toggle(panel("launcher"), left()));
        assert!(open.is_open("launcher", left()) && !open.is_open("launcher", right()));
        assert!(open.toggle(panel("launcher"), left()));
        assert!(!open.is_open("launcher", left()));
        assert!(!open.close("launcher", left()), "closing a closed panel changes nothing");
    }

    #[test]
    fn esc_closes_what_closes_on_it_and_leaves_the_rest() {
        let mut open = OpenPanels::default();
        open.open(notes(), left());
        open.open(panel("launcher"), right());
        assert!(open.dismiss());
        assert!(open.is_open("notes", left()) && !open.is_open("launcher", right()));
        assert!(!open.dismiss());
    }

    #[test]
    fn the_keyboard_is_held_only_on_the_screen_with_a_panel_that_closes_on_esc() {
        let mut open = OpenPanels::default();
        open.open(notes(), left());
        assert!(!open.needs_keyboard(left()) && !open.catches_clicks());
        open.open(panel("launcher"), right());
        assert!(open.needs_keyboard(right()) && !open.needs_keyboard(left()));
        assert!(open.catches_clicks());
        let typing = PanelRules { name: "journal".into(), wants_keyboard: true, ..notes() };
        open.open(typing, left());
        assert!(open.needs_keyboard(left()));
    }

    #[test]
    fn a_screen_that_leaves_takes_its_open_panels_with_it() {
        let mut open = OpenPanels::default();
        open.open(panel("launcher"), left());
        assert!(open.retain(|screen, _| screen != left()));
        assert!(!open.catches_clicks() && !open.any_open(left()));
    }
}
