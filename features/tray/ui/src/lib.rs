//! What the tray draws with GPUI, as its manifest's slots name the views
//! (`features/tray/feature.toml`):
//!
//! - `tray.strip` (host `right-pill.tray`): the chevron and the apps'
//!   icons in the bar's right pill ([`strip`]).
//! - `tray.panel` (host `stage.panels`): the open tray, which the strip
//!   draws in place, so the stage closes it as it does any panel.
//! - `tray.menu` (host `stage.panels`): an app's menu below its icon
//!   ([`menu`]).
//! - `tray.card` (host `quick-settings.cards`): every app in quick
//!   settings, with an app's menu drawn over the drawer ([`card`]).
//!
//! [`model`] is what they work out, without GPUI.

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

/// The open tray's panel draws nothing of its own: the strip draws it.
struct Empty;

impl gpui::Render for Empty {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        div()
    }
}
