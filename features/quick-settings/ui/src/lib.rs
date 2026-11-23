//! What quick settings draws, with GPUI: its button on the right pill
//! (`right-pill.buttons` slot `quick-settings.button`) and the drawer it opens
//! against the screen's right edge (`stage.panels` slot `quick-settings.panel`
//! of `features/quick-settings/feature.toml`).
//!
//! The drawer draws its header and hosts what other features put in it,
//! by their slots: `quick-settings.tiles`, `.sliders`, `.cards`, `.fill`,
//! `.foot` and `.pages`. A feature fills one with a part
//! (`nova_stage::part`), built from the controls in `nova_look` (`Tile`,
//! `Slider`, `ListRow`, `PageHeader`); a tile whose feature draws nothing
//! here yet keeps its place, empty. A page is a part of `.pages` that
//! fills the page's room; a part opens it with
//! `Stage::show_page("quick-settings", screen, Some("<its view id>"))` and
//! its back button returns with `None`.
//!
//! - [`register`]: the one thing `nova-ui` calls.
//! - `button`: the button and its badge.
//! - `drawer`: one screen's drawer, its hosts and motions; `draw` draws it.
//! - `logic`: what the drawer works out, tested alone.
//! - `tokens`: the feature's own sizes.

mod button;
mod draw;
mod drawer;
mod logic;
mod tokens;

use gpui::{App, AppContext, size};
use nova_stage::{Panel, Placement};

/// The drawer's name: the feature's id, which `nova toggle` uses.
pub const NAME: &str = "quick-settings";
/// The view ids the manifest's slots name, and the pill the drawer grows
/// out of.
const BUTTON: &str = "quick-settings.button";
const DRAWER: &str = "quick-settings.panel";
const RIGHT_PILL: &str = "right-pill";

/// Registers the button and the drawer with the stage.
pub fn register(cx: &mut App) {
    nova_stage::part(BUTTON, cx, |screen, cx| cx.new(|cx| button::Button::new(screen, cx)).into());
    nova_stage::panel(DRAWER, cx, |screen, cx| {
        let drawer = cx.new(|cx| drawer::Drawer::new(screen, cx));
        // As tall as the screen, whatever height is given here.
        Panel::new(NAME, size(tokens::WIDTH, screen.size.height), drawer)
            .from(RIGHT_PILL)
            .placement(Placement::RightEdge)
    });
}
