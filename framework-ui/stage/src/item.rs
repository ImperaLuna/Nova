//! What a feature hands the stage: a [`Pill`] for the bar's rows, a
//! [`Panel`] that opens from one or by itself. Both carry only content and
//! the contract's few flags; the stage draws the glass around them, places
//! them, morphs one into the other and keeps the regions.

use crate::rules::PanelRules;
use gpui::{AnyView, FocusHandle, Pixels, SharedString, Size};
use nova_look::tokens::{BAR_GAP, PAD, RADIUS_PANEL};
use std::cell::Cell;
use std::rc::Rc;

/// A pill in one of the bar's rows: glass as tall as the bar and as wide as
/// its content.
pub struct Pill {
    pub(crate) content: AnyView,
    pub(crate) panel: Option<Panel>,
    pub(crate) opens_on_click: bool,
    pub(crate) shown: Option<Shown>,
}

impl Pill {
    /// A pill that shows `content`, a view of the feature's own. The view
    /// lays itself out inside the pill (its padding, its gaps) and redraws
    /// by itself when what it shows changes.
    pub fn new(content: impl Into<AnyView>) -> Self {
        Self { content: content.into(), panel: None, opens_on_click: true, shown: None }
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

    /// For a pill that has nothing to show at times (the central pill with
    /// the launcher, workspaces and clock all off): while `shown` says no,
    /// the stage draws no glass for it, gives it no room in its row and no
    /// region, and a panel that grows out of it rises into place instead.
    /// The view sets it as what it shows changes, and redraws.
    pub fn shown_while(mut self, shown: Shown) -> Self {
        self.shown = Some(shown);
        self
    }
}

/// Whether a pill is shown, set by its own view ([`Pill::shown_while`]).
/// Clones share the one flag.
#[derive(Debug, Clone)]
pub struct Shown(Rc<Cell<bool>>);

impl Shown {
    pub fn new(shown: bool) -> Self {
        Self(Rc::new(Cell::new(shown)))
    }

    /// Returns whether it changed, so the view redraws (`cx.notify()`).
    pub fn set(&self, shown: bool) -> bool {
        self.0.replace(shown) != shown
    }

    pub fn get(&self) -> bool {
        self.0.get()
    }
}

/// A view for a host another feature draws (`central-pill.parts`,
/// `right-pill.buttons`), and how far the host keeps it from its ends.
/// `cx.new(View::new).into()` makes one with the default inset.
pub struct Part {
    pub(crate) view: AnyView,
    pub(crate) inset: Pixels,
}

impl Part {
    pub fn new(view: impl Into<AnyView>) -> Self {
        Self { view: view.into(), inset: PAD }
    }

    /// How far its content sits from the host's end when it is the first
    /// or the last part: [`PAD`] by default, as text wants; round cells
    /// (the workspace numbers) sit closer, as far from the end as from the
    /// pill's top.
    pub fn inset(mut self, inset: Pixels) -> Self {
        self.inset = inset;
        self
    }

    pub fn view(&self) -> &AnyView {
        &self.view
    }

    /// What [`Part::inset`] set.
    pub fn end_inset(&self) -> Pixels {
        self.inset
    }
}

impl<V: gpui::Render> From<gpui::Entity<V>> for Part {
    fn from(view: gpui::Entity<V>) -> Self {
        Self::new(view)
    }
}

/// Where on the screen a panel opens ([`Panel::placement`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Placement {
    /// On the strip across the top of the screen, with the bar: below its
    /// pill, or centered.
    #[default]
    Top,
    /// A drawer against the screen's right edge, top to bottom (quick
    /// settings).
    RightEdge,
    /// Over the whole screen, with no glass: the view draws all there is,
    /// and the stage only fades it in and out (the wallpaper carousel).
    WholeScreen,
}

impl Placement {
    /// Whether the strip across the top draws it. The others are drawn in
    /// a window of their own beside the strip (`crate::panel_window`), kept
    /// ready there and only as big as what is out, so the strip stays as
    /// short as it was.
    pub(crate) fn on_strip(self) -> bool {
        self == Self::Top
    }
}

/// A panel: glass that grows out of a pill, or rises into place without
/// one, and closes again on Esc and on a click outside.
#[derive(Clone)]
pub struct Panel {
    pub(crate) name: SharedString,
    pub(crate) size: Size<Pixels>,
    pub(crate) content: AnyView,
    pub(crate) radius: Pixels,
    pub(crate) top: Pixels,
    pub(crate) placement: Placement,
    pub(crate) from: Option<SharedString>,
    pub(crate) focus: Option<FocusHandle>,
    pub(crate) exclusive: bool,
    pub(crate) wants_keyboard: bool,
    pub(crate) closes_on_click_outside: bool,
    pub(crate) covers_screen: bool,
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
            placement: Placement::Top,
            from: None,
            focus: None,
            exclusive: true,
            wants_keyboard: false,
            closes_on_click_outside: true,
            covers_screen: false,
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

    /// Where it opens; on the top strip by default.
    ///
    /// Against the right edge it is as tall as its screen and as wide as
    /// its `size` (the height given is not used), its right corners past
    /// the edge so only the left ones are round. It still grows out of its
    /// pill, or out of the bar row's right end without one.
    ///
    /// Over the whole screen it is as big as the screen (its `size` is not
    /// used), has no glass and no pill to grow out of, and fades in. A
    /// click anywhere lands on its view, which closes it with
    /// [`crate::Stage::close`] where it wants to.
    ///
    /// Everything else is the same for all three: the flags,
    /// [`crate::Stage::toggle`] and [`crate::Stage::resize`]. Only
    /// [`Panel::top`] is for the top strip alone.
    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
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

    /// A panel over the whole screen that draws all of it itself (the
    /// wallpaper carousel, which covers the screen with its preview): edge
    /// to edge from the screen's top left at the screen's size, whatever
    /// `size` said, with no glass, shadow or blur, and taking every click
    /// on the screen. It fades in and out with the morph, its content
    /// showing all the way.
    pub fn covers_screen(mut self) -> Self {
        self.covers_screen = true;
        self
    }

    pub(crate) fn rules(&self) -> PanelRules {
        PanelRules {
            name: self.name.clone(),
            placement: self.placement,
            exclusive: self.exclusive,
            wants_keyboard: self.wants_keyboard,
            closes_on_click_outside: self.closes_on_click_outside,
        }
    }
}
