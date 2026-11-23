//! What the Bluetooth feature draws, with GPUI: the Bluetooth tile in quick
//! settings (`quick-settings.tiles` slot `bluetooth.tile` of
//! `features/bluetooth/feature.toml`), the Bluetooth page it opens
//! (`quick-settings.pages` slot `bluetooth.page`), and the pop-up that asks a
//! pairing question while the page isn't open (`shell.overlays` slot
//! `bluetooth.pairing`).
//!
//! All of them follow novad's `bluetooth` topic and act with the Bluetooth
//! feature's commands; novad does the work (one connect pairs, trusts and
//! connects a new device), and what happened comes back as the next event.
//!
//! - [`register`]: the one thing `nova-ui` calls.
//! - `tile`, `page` (drawn by `draw`) and `pairing`: the views.
//! - `prompt`: a pairing question and the cards under a row, which the
//!   page and the pop-up share.
//! - `logic`: what the views work out, tested alone.
//! - `tokens`: their own sizes.

mod draw;
mod logic;
mod page;
mod pairing;
mod prompt;
mod tile;
mod tokens;

use gpui::{App, AppContext, Entity};
use nova_bluetooth_api::Command;
use nova_ui_client::Watch;

/// The view ids the manifest's slots name.
const TILE: &str = "bluetooth.tile";
const PAGE: &str = "bluetooth.page";
const PAIRING: &str = "bluetooth.pairing";
/// The drawer the page is in, by its name.
const QUICK_SETTINGS: &str = "quick-settings";

/// Registers the tile, the page and the pop-up with the stage.
pub fn register(cx: &mut App) {
    nova_stage::part(TILE, cx, |screen, cx| cx.new(|cx| tile::BluetoothTile::new(screen, cx)).into());
    nova_stage::part(PAGE, cx, |screen, cx| cx.new(|cx| page::BluetoothPage::new(screen, cx)).into());
    nova_stage::overlay(PAIRING, cx, |cx| Box::new(pairing::Pairing::start(cx)));
}

/// The page shows on some screen: novad looks for nearby devices
/// meanwhile, and the page asks pairing questions in place of the pop-up.
fn watch(cx: &mut App) -> Entity<Watch> {
    Watch::named("bluetooth", |watching| Command::WatchBluetooth { watching }, cx)
}

/// A short stable id for a device's elements, from its address.
fn hash(address: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    address.hash(&mut hasher);
    hasher.finish()
}
