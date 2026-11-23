//! What the notifications feature draws with GPUI, as its manifest's slots
//! name the views (`features/notifications/feature.toml`):
//!
//! - `notifications.popups` (host `shell.overlays`): the pop-ups, with a
//!   window of their own on each screen ([`popups`], drawn by [`window`]
//!   and [`card`]).
//! - `notifications.list` (host `quick-settings.fill`): the list in quick
//!   settings, with clear all and do not disturb ([`list`]).
//! - `notifications.page` (host `quick-settings.pages`): do not disturb and
//!   the per-app switches ([`page`]).
//!
//! [`model`] is their shared link to novad; [`stack`] and [`logic`] are
//! the logic without drawing, each tested alone.

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

/// The settings page's view id, a page of quick settings.
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
