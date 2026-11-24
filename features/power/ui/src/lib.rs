mod button;
mod choice;
mod draw;
mod menu;
mod tokens;

use gpui::AppContext;
use nova_look::tokens::EDGE_TAB_RADIUS;
use nova_stage::{Panel, Placement};

pub const NAME: &str = "power";
const BUTTON: &str = "power.button";
const MENU: &str = "power.menu";

pub fn register(cx: &mut gpui::App) {
    nova_stage::part(BUTTON, cx, |screen, cx| cx.new(|_| button::Button::new(screen)).into());
    nova_stage::panel(MENU, cx, |screen, cx| {
        let menu = cx.new(|cx| menu::Menu::new(screen, cx));
        let (focus, size) = (menu.read(cx).focus(), menu.read(cx).size(cx));
        Panel::new(NAME, size, menu).placement(Placement::RightTab).radius(EDGE_TAB_RADIUS).takes_typing(focus)
    });
}
