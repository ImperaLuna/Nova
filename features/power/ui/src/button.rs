//! The power button on the right pill (`power.button` in
//! `right-pill.buttons`), in the accent: opens and closes the power menu on
//! its screen, which closes quick settings when it's open.
//! `ui/power/PowerButtonSlot.qml` in today's shell.

use crate::NAME;
use gpui::{Context, IntoElement, Render, Window};
use nova_look::{Glyph, PillButton, Theme};
use nova_stage::{Screen, Stage};

pub struct Button {
    screen: Screen,
}

impl Button {
    pub fn new(screen: &Screen) -> Self {
        Self { screen: screen.clone() }
    }
}

impl Render for Button {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let screen = self.screen.id;
        PillButton::new("power", Glyph::POWER)
            .ink(Theme::of(cx).primary)
            .on_click(move |_, _, cx| Stage::toggle(NAME, screen, cx))
    }
}
