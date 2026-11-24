mod app_icon;
mod card;
mod list;
mod logic;
mod model;
mod page;
mod pieces;
mod popups;
mod stack;
mod tokens;
mod window;

use gpui::{App, AppContext};

const PAGE: &str = "notifications.page";

pub fn register(cx: &mut App) {
    nova_stage::overlay("notifications.popups", cx, |cx| Box::new(popups::Popups::start(cx)));
    nova_stage::part("notifications.list", cx, |screen, cx| {
        let screen = screen.clone();
        cx.new(|cx| list::List::new(screen, cx)).into()
    });
    nova_stage::part(PAGE, cx, |screen, cx| {
        let screen = screen.id;
        cx.new(|cx| page::Page::new(screen, cx)).into()
    });
}
