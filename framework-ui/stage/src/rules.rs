use crate::item::Placement;
use gpui::{DisplayId, SharedString};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelRules {
    pub name: SharedString,
    pub placement: Placement,
    pub exclusive: bool,
    pub wants_keyboard: bool,
    pub closes_on_click_outside: bool,
}

impl PanelRules {
    pub fn closes_on_esc(&self) -> bool {
        self.wants_keyboard || self.closes_on_click_outside
    }

    fn ruled_out_by(&self, other: &PanelRules) -> bool {
        other.name == self.name || (other.exclusive && self.exclusive)
    }
}

#[derive(Debug, Default)]
pub struct OpenPanels {
    open: Vec<(DisplayId, PanelRules)>,
    released: Vec<(DisplayId, SharedString)>,
}

impl OpenPanels {
    pub fn is_open(&self, name: &str, screen: DisplayId) -> bool {
        self.open.iter().any(|(on, panel)| *on == screen && panel.name.as_ref() == name)
    }

    pub fn open(&mut self, panel: PanelRules, screen: DisplayId) -> bool {
        if self.is_open(&panel.name, screen) {
            return false;
        }
        self.retain(|_, other| !other.ruled_out_by(&panel));
        self.open.push((screen, panel));
        true
    }

    pub fn close(&mut self, name: &str, screen: DisplayId) -> bool {
        self.retain(|on, panel| !(on == screen && panel.name.as_ref() == name))
    }

    pub fn toggle(&mut self, panel: PanelRules, screen: DisplayId) -> bool {
        if self.is_open(&panel.name, screen) { self.close(&panel.name, screen) } else { self.open(panel, screen) }
    }

    pub fn dismiss(&mut self) -> bool {
        self.retain(|_, panel| !panel.closes_on_esc())
    }

    pub fn retain(&mut self, mut keep: impl FnMut(DisplayId, &PanelRules) -> bool) -> bool {
        let before = self.open.len();
        self.open.retain(|(screen, panel)| keep(*screen, panel));
        let open = &self.open;
        self.released.retain(|(screen, name)| open.iter().any(|(on, panel)| on == screen && panel.name == *name));
        self.open.len() != before
    }

    pub fn release_keyboard(&mut self, name: &str, screen: DisplayId) -> bool {
        if !self.is_open(name, screen) || self.has_released(name, screen) {
            return false;
        }
        self.released.push((screen, SharedString::from(name.to_owned())));
        true
    }

    pub fn reclaim_keyboard(&mut self, name: &str, screen: DisplayId) -> bool {
        let before = self.released.len();
        self.released.retain(|(on, released)| !(*on == screen && released.as_ref() == name));
        self.released.len() != before
    }

    fn has_released(&self, name: &str, screen: DisplayId) -> bool {
        self.released.iter().any(|(on, released)| *on == screen && released.as_ref() == name)
    }

    pub fn needs_keyboard(&self, screen: DisplayId, on_strip: bool) -> bool {
        let here = |on: DisplayId, panel: &PanelRules| on == screen && panel.placement.on_strip() == on_strip;
        self.open
            .iter()
            .any(|(on, panel)| here(*on, panel) && panel.closes_on_esc() && !self.has_released(&panel.name, *on))
    }

    pub fn any_open(&self, screen: DisplayId) -> bool {
        self.open.iter().any(|(on, _)| *on == screen)
    }

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
        PanelRules {
            name: name.into(),
            placement: Placement::Top,
            exclusive: true,
            wants_keyboard: false,
            closes_on_click_outside: true,
        }
    }

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
        let top = true;
        assert!(!open.needs_keyboard(left(), top) && !open.catches_clicks());
        open.open(panel("launcher"), right());
        assert!(open.needs_keyboard(right(), top) && !open.needs_keyboard(left(), top));
        assert!(open.catches_clicks());
        let typing = PanelRules { name: "journal".into(), wants_keyboard: true, ..notes() };
        open.open(typing, left());
        assert!(open.needs_keyboard(left(), top));
    }

    #[test]
    fn an_open_panel_can_hand_the_keyboard_back_until_it_closes() {
        let mut open = OpenPanels::default();
        let top = true;
        assert!(!open.release_keyboard("launcher", left()), "only an open panel has a keyboard to hand back");
        open.open(panel("launcher"), left());
        assert!(open.release_keyboard("launcher", left()));
        assert!(!open.needs_keyboard(left(), top) && open.is_open("launcher", left()));
        assert!(open.reclaim_keyboard("launcher", left()));
        assert!(open.needs_keyboard(left(), top));
        open.release_keyboard("launcher", left());
        open.close("launcher", left());
        open.open(panel("launcher"), left());
        assert!(open.needs_keyboard(left(), top), "opening again takes the keyboard again");
        open.release_keyboard("launcher", left());
        open.open(panel("power"), right());
        open.open(panel("launcher"), left());
        assert!(open.needs_keyboard(left(), top), "also after another panel closed it");
    }

    #[test]
    fn a_drawer_at_the_edge_and_a_panel_on_the_strip_close_each_other() {
        let drawer = || PanelRules { placement: Placement::RightEdge, ..panel("quick-settings") };
        let mut open = OpenPanels::default();
        open.open(panel("launcher"), left());
        assert!(open.open(drawer(), left()));
        assert!(!open.is_open("launcher", left()), "the drawer closes the strip's panel");
        assert!(open.needs_keyboard(left(), false), "its own window holds the keyboard");
        assert!(!open.needs_keyboard(left(), true), "and the strip's lets go");
        assert!(open.open(panel("launcher"), right()));
        assert!(!open.is_open("quick-settings", left()), "and the other way round, across screens");
        assert!(!open.needs_keyboard(left(), false));
    }

    #[test]
    fn a_screen_that_leaves_takes_its_open_panels_with_it() {
        let mut open = OpenPanels::default();
        open.open(panel("launcher"), left());
        assert!(open.retain(|screen, _| screen != left()));
        assert!(!open.catches_clicks() && !open.any_open(left()));
    }
}
