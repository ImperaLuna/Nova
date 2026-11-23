//! The right pill's frame (view `right-pill` in `bar.right`), ported from
//! `ui/bar/RightPill.qml`: the tray of `right-pill.tray` while some app
//! sits in it, a divider, then the buttons of `right-pill.buttons` (quick
//! settings, power) left to right by their slot order, a button's inset
//! from the pill's ends. The stage leaves it out while it has neither.

use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Subscription, Window, div};
use nova_look::Theme;
use nova_look::tokens::{DIVIDER_HEIGHT, DIVIDER_WIDTH, PILL_BUTTON_GAP, PILL_BUTTON_INSET};
use nova_stage::{Part, Parts, Pill, Screen, Shown, Stage};

const VIEW: &str = "right-pill";
/// The host its tray comes from: shown while an app sits in it.
const TRAY: &str = "right-pill.tray";
/// The host its buttons come from.
const BUTTONS: &str = "right-pill.buttons";

pub(crate) fn register(cx: &mut App) {
    nova_stage::pill(VIEW, cx, |screen, cx| {
        let shown = Shown::any(Vec::new());
        let pill = cx.new(|cx| RightPill::new(screen, shown.clone(), cx));
        // Each button opens its own panel.
        Pill::new(pill).opens_on_click(false).shown_while(shown)
    });
}

struct RightPill {
    tray: Parts,
    buttons: Parts,
    /// Shown while it has a button or a tray part is shown.
    shown: Shown,
    _slots: Option<Subscription>,
}

impl RightPill {
    fn new(screen: &Screen, shown: Shown, cx: &mut Context<Self>) -> Self {
        let slots = Stage::slots(cx).map(|slots| {
            cx.observe(&slots, |pill, _, cx| {
                pill.tray.sync(cx);
                pill.buttons.sync(cx);
                pill.follow();
                cx.notify();
            })
        });
        let tray = Parts::new(TRAY, screen, cx);
        let buttons = Parts::new(BUTTONS, screen, cx);
        let pill = Self { tray, buttons, shown, _slots: slots };
        pill.follow();
        pill
    }

    /// Shown with its buttons, or a tray part: an app in the tray.
    fn follow(&self) {
        let buttons = Shown::new(!self.buttons.is_empty());
        self.shown.follow(self.tray.parts().map(Part::shown).chain([buttons]).collect());
    }
}

impl Render for RightPill {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tray: Vec<_> = self.tray.parts().filter(|part| part.is_shown()).map(|part| part.view().clone()).collect();
        let has_buttons = !self.buttons.is_empty();
        let divider = (!tray.is_empty() && has_buttons)
            .then(|| div().w(DIVIDER_WIDTH).h(DIVIDER_HEIGHT).flex_none().bg(Theme::of(cx).glass_rim()));
        div()
            .h_full()
            .flex()
            .items_center()
            .px(PILL_BUTTON_INSET)
            .gap(PILL_BUTTON_GAP)
            .children(tray)
            .children(divider)
            .children(self.buttons.views().cloned())
    }
}
