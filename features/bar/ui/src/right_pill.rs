//! The right pill's frame (view `right-pill` in `bar.right`), ported from
//! `ui/bar/RightPill.qml`: the tray of `right-pill.tray` while some app
//! sits in it, a divider, then the buttons of `right-pill.buttons` (quick
//! settings, power) left to right by their slot order, a button's inset
//! from the pill's ends. The stage leaves it out while it has neither.

use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Subscription, Window, div};
use nova_look::Theme;
use nova_look::tokens::{DIVIDER_HEIGHT, DIVIDER_WIDTH, PILL_BUTTON_GAP, PILL_BUTTON_INSET};
use nova_stage::{Parts, Pill, Screen, Shown, Stage};

const VIEW: &str = "right-pill";
/// The host its tray comes from: shown while an app sits in it.
const TRAY: &str = "right-pill.tray";
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
    tray: Parts,
    buttons: Parts,
    shown: Shown,
    _slots: Option<Subscription>,
}

impl RightPill {
    fn new(screen: &Screen, shown: Shown, cx: &mut Context<Self>) -> Self {
        let slots = Stage::slots(cx).map(|slots| {
            cx.observe(&slots, |pill, _, cx| {
                pill.tray.sync(cx);
                pill.buttons.sync(cx);
                cx.notify();
            })
        });
        let tray = Parts::new(TRAY, screen, cx);
        let buttons = Parts::new(BUTTONS, screen, cx);
        shown.set(!buttons.is_empty() || has_tray(&tray));
        Self { tray, buttons, shown, _slots: slots }
    }
}

/// Whether some tray part has something to show: an app in the tray.
fn has_tray(tray: &Parts) -> bool {
    tray.parts().any(|part| part.is_shown())
}

impl Render for RightPill {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tray: Vec<_> = self.tray.parts().filter(|part| part.is_shown()).map(|part| part.view().clone()).collect();
        let has_buttons = !self.buttons.is_empty();
        // A tray app came or went: the stage reads whether the pill shows
        // before this draws, so it looks again on the next frame.
        if self.shown.set(has_buttons || !tray.is_empty()) {
            window.request_animation_frame();
        }
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
