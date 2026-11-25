mod button;
mod buttons;
mod draw;
mod drawer;
mod logic;
mod tokens;

use gpui::{App, AppContext, size};
use nova_stage::{Panel, Placement};

pub const NAME: &str = "quick-settings";
const BUTTON: &str = "quick-settings.button";
const DRAWER: &str = "quick-settings.panel";
const RIGHT_PILL: &str = "right-pill";

pub fn register(cx: &mut App) {
    nova_stage::part(BUTTON, cx, |screen, cx| cx.new(|cx| button::Button::new(screen, cx)).into());
    nova_stage::panel(DRAWER, cx, |screen, cx| {
        let buttons = cx.new(|cx| buttons::Buttons::new(screen, cx));
        let drawer = cx.new(|cx| drawer::Drawer::new(screen, buttons.clone(), cx));
        Panel::new(NAME, size(tokens::WIDTH, screen.size.height), drawer)
            .steady(buttons)
            .from(RIGHT_PILL)
            .placement(Placement::RightEdge)
    });
}
