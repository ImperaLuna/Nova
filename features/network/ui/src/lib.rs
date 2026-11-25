mod draw;
mod logic;
mod page;
mod tile;
mod tokens;

use gpui::{App, AppContext};
use nova_network_api::Command;
use nova_stage::Part;

const TILE: &str = "network.tile";
const PAGE: &str = "network.page";
const QUICK_SETTINGS: &str = "quick-settings";
const TRAFFIC: &str = "network-traffic";

fn watch_traffic(watching: bool) -> Command {
    Command::WatchNetworkTraffic { watching }
}

pub fn register(cx: &mut App) {
    nova_stage::part(TILE, cx, |screen, cx| {
        let tile = cx.new(|cx| tile::NetworkTile::new(screen, cx));
        let shown = tile.read(cx).shown();
        Part::new(tile).shown_while(shown)
    });
    nova_stage::part(PAGE, cx, |screen, cx| cx.new(|cx| page::NetworkPage::new(screen, cx)).into());
}
