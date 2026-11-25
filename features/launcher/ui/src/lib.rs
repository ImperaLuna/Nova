mod draw;
mod launcher;
mod order;
mod rows;
mod tokens;

use gpui::{App, AppContext};
use launcher::{Launcher, NAME, top};
use nova_stage::Panel;

pub fn register(cx: &mut App) {
    nova_stage::panel("launcher.panel", cx, |screen, cx| {
        let launcher = cx.new(|cx| Launcher::new(screen, cx));
        let (focus, size) = (launcher.read(cx).focus(cx), launcher.read(cx).size(cx));
        Panel::new(NAME, size, launcher).top(top(screen)).takes_typing(focus)
    });
}
