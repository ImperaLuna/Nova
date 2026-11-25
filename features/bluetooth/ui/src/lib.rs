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

const TILE: &str = "bluetooth.tile";
const PAGE: &str = "bluetooth.page";
const PAIRING: &str = "bluetooth.pairing";
const QUICK_SETTINGS: &str = "quick-settings";

pub fn register(cx: &mut App) {
    nova_stage::part(TILE, cx, |screen, cx| cx.new(|cx| tile::BluetoothTile::new(screen, cx)).into());
    nova_stage::part(PAGE, cx, |screen, cx| cx.new(|cx| page::BluetoothPage::new(screen, cx)).into());
    nova_stage::overlay(PAIRING, cx, |cx| Box::new(pairing::Pairing::start(cx)));
}

fn watch(cx: &mut App) -> Entity<Watch> {
    Watch::named("bluetooth", |watching| Command::WatchBluetooth { watching }, cx)
}

fn hash(address: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    address.hash(&mut hasher);
    hasher.finish()
}
