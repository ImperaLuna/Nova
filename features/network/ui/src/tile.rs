use crate::{PAGE, QUICK_SETTINGS, logic};
use gpui::{Context, Entity, IntoElement, Render, Subscription, Window};
use nova_look::Tile;
use nova_network_api::{Event, Network, TOPIC};
use nova_stage::{Screen, Shown, Stage};
use nova_ui_client::Live;

pub struct NetworkTile {
    screen: Screen,
    network: Entity<Live<Network>>,
    shown: Shown,
    _followed: Subscription,
}

impl NetworkTile {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let network = Live::follow(TOPIC, cx, |event| match event {
            Event::Network(network) => Some(network),
            Event::NetworkTraffic(_) => None,
        });
        let shown = Shown::new(network.read(cx).get().is_some());
        let followed = cx.observe(&network, |tile, network, cx| {
            tile.shown.set(network.read(cx).get().is_some(), cx);
            cx.notify();
        });
        Self { screen: screen.clone(), network, shown, _followed: followed }
    }

    pub fn shown(&self) -> Shown {
        self.shown.clone()
    }
}

impl Render for NetworkTile {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let network = self.network.read(cx).get();
        let icon = logic::tile_icon(network);
        let screen = self.screen.id;
        let open =
            move |_: &_, _: &mut Window, cx: &mut gpui::App| Stage::show_page(QUICK_SETTINGS, screen, Some(PAGE), cx);
        let tile =
            Tile::new("network", icon.glyph, "Network").on(logic::tile_on(network)).on_toggle(open).on_open(open);
        match icon.fill {
            Some(fill) => tile.filled(fill),
            None => tile,
        }
    }
}
