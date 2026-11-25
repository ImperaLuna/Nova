use gpui::{Context, IntoElement, ParentElement, Render, Styled, Subscription, Window, div};
use nova_look::PILL_BUTTONS;
use nova_look::tokens::{BAR_GAP, BAR_HEIGHT, BAR_MARGIN, PILL_BUTTON_GAP, PILL_BUTTON_INSET};
use nova_stage::{Parts, Screen, Shown, Stage};

pub struct Buttons {
    pub parts: Parts,
    _followed: Vec<Subscription>,
}

impl Buttons {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let mut followed: Vec<Subscription> = Stage::slots(cx)
            .map(|slots| {
                cx.observe(&slots, |buttons: &mut Self, _, cx| {
                    if buttons.parts.sync(cx) {
                        cx.notify();
                    }
                })
            })
            .into_iter()
            .collect();
        followed.push(Shown::observe(cx, |buttons, cx| {
            if buttons.parts.shown_changed() {
                cx.notify();
            }
        }));
        Self { parts: Parts::new(PILL_BUTTONS, screen, cx), _followed: followed }
    }

    pub fn count(&self) -> usize {
        self.parts.views().count()
    }
}

impl Render for Buttons {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().relative().child(
            div()
                .absolute()
                .top(BAR_GAP)
                .right(BAR_MARGIN)
                .h(BAR_HEIGHT)
                .px(PILL_BUTTON_INSET)
                .flex()
                .items_center()
                .gap(PILL_BUTTON_GAP)
                .children(self.parts.views().cloned()),
        )
    }
}
