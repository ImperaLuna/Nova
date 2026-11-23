//! What the launcher draws, with GPUI: the stage's `launcher` panel
//! (`stage.panels` slot `launcher.panel` of `features/launcher/feature.toml`).
//!
//! - [`register`]: the one thing `nova-ui` calls.
//! - `launcher`: the panel's view, one per screen: its state, its keys,
//!   what it asks novad.
//! - `draw`: how it looks.
//! - `rows`: what the list shows and where each row sits, tested alone.
//! - `tokens`: the launcher's own sizes.

mod draw;
mod launcher;
mod rows;
mod tokens;

use gpui::{App, AppContext};
use launcher::{Launcher, NAME, top};
use nova_stage::Panel;

/// Registers the launcher's panel with the stage. It fades in where it
/// stands, a share of the screen down, and takes the keyboard for its
/// search field while it is open.
pub fn register(cx: &mut App) {
    nova_stage::panel("launcher.panel", cx, |screen, cx| {
        let launcher = cx.new(|cx| Launcher::new(screen, cx));
        let (focus, size) = (launcher.read(cx).focus(cx), launcher.read(cx).size(cx));
        Panel::new(NAME, size, launcher).top(top(screen)).takes_typing(focus)
    });
}
