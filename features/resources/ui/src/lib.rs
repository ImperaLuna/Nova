//! What the resources feature draws, with GPUI, in quick settings: the card
//! of rings at the drawer's foot (`quick-settings.foot` slot
//! `resources.card` of `features/resources/feature.toml`) and the page it
//! opens (`quick-settings.pages` slot `resources.page`).
//! `ResourcesCard.qml` and `ResourcesPage.qml` in `ui/quick_settings` in
//! today's shell, which stay the spec until it is removed.
//!
//! novad samples the machine only while some connection watches
//! (`watch_resources`): this program watches while any screen's drawer
//! shows the card or the page (a `nova_ui_client::Watch`, kept by the
//! card).
//!
//! - [`register`]: the one thing `nova-ui` calls.
//! - `card` (with `ring`), `page` (with `graph`): the two views.
//! - `logic`: what the QML and its `.js` worked out, tested alone.
//! - `tokens`: the feature's own sizes.

mod card;
mod graph;
mod logic;
mod page;
mod ring;
mod tokens;

use gpui::{App, AppContext};

/// The view ids the manifest's slots name.
const CARD: &str = "resources.card";
const PAGE: &str = "resources.page";
/// The drawer they are in, by its name.
const QUICK_SETTINGS: &str = "quick-settings";

/// Registers the card and the page with the stage.
pub fn register(cx: &mut App) {
    nova_stage::part(CARD, cx, |screen, cx| cx.new(|cx| card::Card::new(screen, cx)).into());
    nova_stage::part(PAGE, cx, |screen, cx| cx.new(|cx| page::Page::new(screen, cx)).into());
}
