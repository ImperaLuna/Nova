//! What the network feature draws, with GPUI, in quick settings: the
//! network tile (`quick-settings.tiles` slot `network.tile` of
//! `features/network/feature.toml`) and the network page it opens
//! (`quick-settings.pages` slot `network.page`). `NetworkTile.qml`,
//! `NetworkPage.qml` and `WifiIcon.qml` in `ui/quick_settings` in today's
//! shell, which stay the spec until it is removed.
//!
//! Both follow novad's `network` topic and change things with the network
//! feature's commands; what they show comes back as the next event. While
//! the page shows on some screen, novad sends the traffic each second
//! (`network_traffic`), and the page asks for a scan each time it opens.
//!
//! - [`register`]: the one thing `nova-ui` calls.
//! - `tile`, `page`: the two views; `draw` draws the page.
//! - `logic`: what `network.js` worked out, tested alone.
//! - `tokens`: the page's own sizes.

mod draw;
mod logic;
mod page;
mod tile;
mod tokens;

use gpui::{App, AppContext};
use nova_network_api::Command;
use nova_stage::Part;

/// The view ids the manifest's slots name.
const TILE: &str = "network.tile";
const PAGE: &str = "network.page";
/// The drawer the page is in, by its name.
const QUICK_SETTINGS: &str = "quick-settings";
/// novad's traffic each second, while a page shows it.
const TRAFFIC: &str = "network-traffic";

fn watch_traffic(watching: bool) -> Command {
    Command::WatchNetworkTraffic { watching }
}

/// Registers the tile and the page with the stage.
pub fn register(cx: &mut App) {
    nova_stage::part(TILE, cx, |screen, cx| {
        let tile = cx.new(|cx| tile::NetworkTile::new(screen, cx));
        let shown = tile.read(cx).shown();
        Part::new(tile).shown_while(shown)
    });
    nova_stage::part(PAGE, cx, |screen, cx| cx.new(|cx| page::NetworkPage::new(screen, cx)).into());
}
