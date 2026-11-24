use gpui::{App, AppContext, Context, IntoElement, ParentElement, Render, Styled, Subscription, Window, div};
use nova_look::Theme;
use nova_look::tokens::{DIVIDER_HEIGHT, DIVIDER_WIDTH, PILL_BUTTON_GAP, PILL_BUTTON_INSET};
use nova_stage::{Part, Parts, Pill, Screen, Shown, Stage};

const VIEW: &str = "right-pill";
const TRAY: &str = "right-pill.tray";
const BUTTONS: &str = "right-pill.buttons";

pub(crate) fn register(cx: &mut App) {
    nova_stage::pill(VIEW, cx, |screen, cx| {
        let shown = Shown::any(Vec::new());
        let pill = cx.new(|cx| RightPill::new(screen, shown.clone(), cx));
        Pill::new(pill).opens_on_click(false).shown_while(shown)
    });
}

struct RightPill {
    tray: Parts,
    buttons: Parts,
    shown: Shown,
    _observers: Vec<Subscription>,
}

impl RightPill {
    fn new(screen: &Screen, shown: Shown, cx: &mut Context<Self>) -> Self {
        let slots = Stage::slots(cx).map(|slots| {
            cx.observe(&slots, |pill, _, cx| {
                pill.tray.sync(cx);
                pill.buttons.sync(cx);
                pill.follow(cx);
                cx.notify();
            })
        });
        let tray_shown = Shown::observe(cx, |pill, cx| {
            if pill.tray.shown_changed() {
                cx.notify();
            }
        });
        let tray = Parts::new(TRAY, screen, cx);
        let buttons = Parts::new(BUTTONS, screen, cx);
        let pill = Self { tray, buttons, shown, _observers: slots.into_iter().chain([tray_shown]).collect() };
        pill.follow(cx);
        pill
    }

    fn follow(&self, cx: &mut App) {
        let buttons = Shown::new(!self.buttons.is_empty());
        self.shown.follow(self.tray.parts().map(Part::shown).chain([buttons]).collect(), cx);
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
