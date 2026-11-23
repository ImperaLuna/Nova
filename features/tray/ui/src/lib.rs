mod card;
mod menu;
mod model;
mod strip;
mod tokens;

use gpui::{App, AppContext, div, size};
use nova_stage::{Panel, Part, Shown};

pub fn register(cx: &mut App) {
    nova_stage::part("tray.strip", cx, |screen, cx| {
        let shown = Shown::new(false);
        let strip = cx.new(|cx| strip::Strip::new(screen, shown.clone(), cx));
        Part::new(strip).shown_while(shown)
    });
    nova_stage::panel("tray.panel", cx, |_, cx| {
        Panel::new(strip::TRAY, size(gpui::px(0.0), gpui::px(0.0)), cx.new(|_| Empty)).in_place()
    });
    nova_stage::panel("tray.menu", cx, menu::panel);
    nova_stage::part("tray.card", cx, |screen, cx| {
        let shown = Shown::new(false);
        let card = cx.new(|cx| card::Card::new(screen, shown.clone(), cx));
        Part::new(card).shown_while(shown)
    });
}

struct Empty;

impl gpui::Render for Empty {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        div()
    }
}
