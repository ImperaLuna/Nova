//! What the power menu draws, with GPUI: its button on the right pill
//! (`right-pill.buttons` slot `power.button`) and the menu it opens, a tab
//! against the right screen edge, halfway down (`stage.panels` slot
//! `power.menu` of `features/power/feature.toml`). `ui/power` in today's
//! shell, which stays the spec until it is removed.
//!
//! The menu asks novad what it can do each time it opens (`power_options`)
//! and has it run an action (`power`): lock (the lock screen, as `nova
//! lock` does), log out (the compositor), suspend, restart and shut down
//! (logind). The stage draws its glass ([`Placement::RightTab`]) and keeps
//! the keyboard on it while it is open.
//!
//! - [`register`]: the one thing `nova-ui` calls.
//! - `button`: the button on the right pill.
//! - `menu`: one screen's menu, what it asks novad and when; `draw` draws
//!   it.
//! - `choice`: what its clicks and keys pick (`PowerChoice.qml`), tested
//!   alone.
//! - `tokens`: the feature's own sizes.

mod button;
mod choice;
mod draw;
mod menu;
mod tokens;

use gpui::AppContext;
use nova_look::tokens::EDGE_TAB_RADIUS;
use nova_stage::{Panel, Placement};

/// The menu's name: the feature's id, which `nova toggle` uses.
pub const NAME: &str = "power";
/// The view ids the manifest's slots name.
const BUTTON: &str = "power.button";
const MENU: &str = "power.menu";

/// Registers the button and the menu with the stage.
pub fn register(cx: &mut gpui::App) {
    nova_stage::part(BUTTON, cx, |screen, cx| cx.new(|_| button::Button::new(screen)).into());
    nova_stage::panel(MENU, cx, |screen, cx| {
        let menu = cx.new(|cx| menu::Menu::new(screen, cx));
        let (focus, size) = (menu.read(cx).focus(), menu.read(cx).size(cx));
        Panel::new(NAME, size, menu).placement(Placement::RightTab).radius(EDGE_TAB_RADIUS).takes_typing(focus)
    });
}
