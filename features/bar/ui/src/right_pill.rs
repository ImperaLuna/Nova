//! The right pill's frame (view `right-pill` in `bar.right`), ported from
//! `ui/bar/RightPill.qml`: the buttons of `right-pill.buttons` (quick
//! settings, power) left to right by their slot order, a button's inset
//! from the pill's ends. The stage leaves it out while no button's feature
//! is ready. The tray's chevron and strip join it with the tray's port.

use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Subscription, Window, div};
use nova_look::tokens::{PILL_BUTTON_GAP, PILL_BUTTON_INSET};
use nova_stage::{Parts, Pill, Screen, Shown, Stage};

const VIEW: &str = "right-pill";
/// The host its buttons come from.
const BUTTONS: &str = "right-pill.buttons";

pub(crate) fn register(cx: &mut App) {
    nova_stage::pill(VIEW, cx, |screen, cx| {
        let shown = Shown::new(false);
        let pill = cx.new(|cx| RightPill::new(screen, shown.clone(), cx));
        // Each button opens its own panel.
        Pill::new(pill).opens_on_click(false).shown_while(shown)
    });
}

struct RightPill {
    buttons: Parts,
    shown: Shown,
    _slots: Option<Subscription>,
}

impl RightPill {
    fn new(screen: &Screen, shown: Shown, cx: &mut Context<Self>) -> Self {
        let slots = Stage::slots(cx).map(|slots| {
            cx.observe(&slots, |pill, _, cx| {
                pill.buttons.sync(cx);
                pill.shown.set(!pill.buttons.is_empty());
                cx.notify();
            })
        });
        let buttons = Parts::new(BUTTONS, screen, cx);
        shown.set(!buttons.is_empty());
        Self { buttons, shown, _slots: slots }
    }
}

impl Render for RightPill {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_full()
            .flex()
            .items_center()
            .px(PILL_BUTTON_INSET)
            .gap(PILL_BUTTON_GAP)
            .children(self.buttons.views().cloned())
    }
}
