//! `bluetooth.tile`: the Bluetooth tile. A click turns Bluetooth on when
//! it's off, and opens the page when it's on: turning it off is the page's
//! switch, so a click meant for the page doesn't drop the headphones.
//! Turned off by a hardware switch, it opens the page, which says so.
//! `BluetoothTile.qml` in today's shell.

use crate::{PAGE, QUICK_SETTINGS};
use gpui::{App, Context, Entity, IntoElement, Render, Subscription, Window};
use nova_bluetooth_api::{Bluetooth, BluetoothRadio, Command, Event, TOPIC};
use nova_look::{Glyph, Tile};
use nova_stage::{Screen, Stage};
use nova_ui_client::{Daemon, Live};

pub struct BluetoothTile {
    screen: Screen,
    bluetooth: Entity<Live<Bluetooth>>,
    _followed: Subscription,
}

impl BluetoothTile {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let bluetooth = Live::follow(TOPIC, cx, |Event::Bluetooth(bluetooth)| Some(bluetooth));
        let followed = cx.observe(&bluetooth, |_, _, cx| cx.notify());
        Self { screen: screen.clone(), bluetooth, _followed: followed }
    }
}

impl Render for BluetoothTile {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let radio = self.bluetooth.read(cx).get().map_or(BluetoothRadio::Missing, |state| state.radio);
        let screen = self.screen.id;
        let open = move |cx: &mut App| Stage::show_page(QUICK_SETTINGS, screen, Some(PAGE), cx);
        Tile::new("bluetooth", Glyph::BLUETOOTH, "Bluetooth")
            .on(radio == BluetoothRadio::On)
            .available(radio != BluetoothRadio::Missing)
            .on_toggle(move |_, _, cx| match radio {
                BluetoothRadio::Off => Daemon::send(Command::BluetoothPower { on: true }, cx),
                _ => open(cx),
            })
            .on_open(move |_, _, cx| open(cx))
    }
}
