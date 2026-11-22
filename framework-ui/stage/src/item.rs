//! What a feature hands the stage: a [`Pill`] for the bar's rows, a
//! [`Panel`] that opens from one or by itself. Both carry only content and
//! the contract's few flags; the stage draws the glass around them, places
//! them, morphs one into the other and keeps the regions.

use crate::rules::PanelRules;
use gpui::{AnyView, FocusHandle, Pixels, SharedString, Size};
use nova_look::tokens::{BAR_GAP, RADIUS_PANEL};

/// A pill in one of the bar's rows: glass as tall as the bar and as wide as
/// its content.
pub struct Pill {
    pub(crate) content: AnyView,
    pub(crate) panel: Option<Panel>,
    pub(crate) opens_on_click: bool,
}

impl Pill {
    /// A pill that shows `content`, a view of the feature's own. The view
    /// lays itself out inside the pill (its padding, its gaps) and redraws
    /// by itself when what it shows changes.
    pub fn new(content: impl Into<AnyView>) -> Self {
        Self { content: content.into(), panel: None, opens_on_click: true }
    }

    /// The panel this pill grows into. A click on the pill opens it, unless
    /// [`Pill::opens_on_click`] says otherwise.
    pub fn panel(mut self, panel: Panel) -> Self {
        self.panel = Some(panel);
        self
    }

    /// `false` for a pill whose own buttons decide what a click does (the
    /// central pill's search button opens the launcher, its numbers switch
    /// workspace): it calls [`crate::Stage::toggle`] itself.
    pub fn opens_on_click(mut self, opens: bool) -> Self {
        self.opens_on_click = opens;
        self
    }
}

/// A panel: glass that grows out of a pill, or rises into place without
/// one, and closes again on Esc and on a click outside.
pub struct Panel {
    pub(crate) name: SharedString,
    pub(crate) size: Size<Pixels>,
    pub(crate) content: AnyView,
    pub(crate) radius: Pixels,
    pub(crate) top: Pixels,
    pub(crate) from: Option<SharedString>,
    pub(crate) focus: Option<FocusHandle>,
    pub(crate) exclusive: bool,
    pub(crate) wants_keyboard: bool,
    pub(crate) closes_on_click_outside: bool,
}

impl Panel {
    /// A panel called `name`: its feature's id, which is also what `nova
    /// toggle` and [`crate::Stage::toggle`] call it. `size` is its size
    /// once open, and `content` a view of the feature's own that fills it.
    /// The stage shows the content only once the panel has grown, so the
    /// view draws its open look and nothing else.
    pub fn new(name: impl Into<SharedString>, size: Size<Pixels>, content: impl Into<AnyView>) -> Self {
        Self {
            name: name.into(),
            size,
            content: content.into(),
            radius: RADIUS_PANEL,
            top: BAR_GAP,
            from: None,
            focus: None,
            exclusive: true,
            wants_keyboard: false,
            closes_on_click_outside: true,
        }
    }

    /// The pill it grows out of, by that pill's view id, for a panel that
    /// is no pill's own (the launcher out of the central pill). With that
    /// pill not on screen it rises into place instead.
    pub fn from(mut self, pill_view: impl Into<SharedString>) -> Self {
        self.from = Some(pill_view.into());
        self
    }

    /// How far down the screen its top edge is once open; the bar's row by
    /// default.
    pub fn top(mut self, top: Pixels) -> Self {
        self.top = top;
        self
    }

    /// Its corners once open; a panel's by default.
    pub fn radius(mut self, radius: Pixels) -> Self {
        self.radius = radius;
        self
    }

    /// It takes typing (the launcher's search field): `focus` gets the
    /// keyboard each time it opens.
    pub fn takes_typing(mut self, focus: FocusHandle) -> Self {
        self.wants_keyboard = true;
        self.focus = Some(focus);
        self
    }

    /// `false` for a panel that stays open beside the others (a notes
    /// window would); every other panel closes when another opens.
    pub fn exclusive(mut self, exclusive: bool) -> Self {
        self.exclusive = exclusive;
        self
    }

    /// `false` for a panel that Esc and a click outside leave open.
    pub fn closes_on_click_outside(mut self, closes: bool) -> Self {
        self.closes_on_click_outside = closes;
        self
    }

    pub(crate) fn rules(&self) -> PanelRules {
        PanelRules {
            name: self.name.clone(),
            exclusive: self.exclusive,
            wants_keyboard: self.wants_keyboard,
            closes_on_click_outside: self.closes_on_click_outside,
        }
    }
}
