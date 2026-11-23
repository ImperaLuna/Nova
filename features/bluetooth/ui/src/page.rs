//! `bluetooth.page`: the Bluetooth page's state and what it asks of novad.
//! `draw.rs` draws it.
//!
//! While it shows on some screen, novad looks for nearby devices and asks
//! pairing questions here instead of on the pop-up ([`crate::watch`]). It
//! opens fresh each time quick settings shows it: no menu, no confirm, no
//! failure closed. What's typed into a PIN field stays while the rows
//! around it move, and goes with the question.

use crate::{PAGE, QUICK_SETTINGS, logic};
use gpui::{AppContext, Context, Entity, KeyDownEvent, Subscription, Window};
use nova_bluetooth_api::{Bluetooth, BluetoothDevice, BluetoothRadio, Command, Event, TOPIC};
use nova_look::motion::Eased;
use nova_look::tokens::DURATION_COLOR;
use nova_look::{FieldEvent, TextField};
use nova_stage::{Screen, Stage};
use nova_ui_client::{Daemon, Live, Watch};
use std::collections::HashMap;
use std::time::Instant;

pub struct BluetoothPage {
    pub screen: Screen,
    pub bluetooth: Entity<Live<Bluetooth>>,
    watch: Entity<Watch>,
    /// Quick settings shows this page on its screen.
    shown: bool,
    /// The connected device whose Disconnect and Forget show.
    pub opened: Option<String>,
    /// The known device asking to confirm Forget.
    pub forgetting: Option<String>,
    /// Failures closed with Close, by address: hidden until the next one.
    pub closed: HashMap<String, String>,
    /// The row under the pointer, by address.
    pub hovered: Option<String>,
    /// A PIN being typed for the question asked now.
    pub pin: Entity<TextField>,
    /// The question whose field was given the focus.
    pub focused_for: Option<u64>,
    /// The question the PIN field was last typed for.
    pin_for: Option<u64>,
    /// Where the switch's knob is, sliding as it flips.
    pub power_knob: Eased,
    _followed: Vec<Subscription>,
}

impl BluetoothPage {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let bluetooth = Live::follow(TOPIC, cx, |Event::Bluetooth(bluetooth)| Some(bluetooth));
        let pin = cx.new(TextField::new);
        let mut followed = vec![
            cx.observe(&bluetooth, |page, _, cx| page.follow_bluetooth(cx)),
            cx.subscribe(&pin, |_, _, FieldEvent::Changed, cx| cx.notify()),
        ];
        followed.extend(Stage::observe_panels(cx, |page, cx| page.follow_stage(cx)));
        let watch = crate::watch(cx);
        let me = cx.entity_id();
        cx.on_release({
            let watch = watch.clone();
            move |_, cx| Watch::want(&watch, me, false, cx)
        })
        .detach();
        let on = is_on(bluetooth.read(cx).get());
        Self {
            screen: screen.clone(),
            bluetooth,
            watch,
            shown: false,
            opened: None,
            forgetting: None,
            closed: HashMap::new(),
            hovered: None,
            pin,
            focused_for: None,
            pin_for: None,
            power_knob: Eased::lasting(on, DURATION_COLOR, Instant::now()),
            _followed: followed,
        }
    }

    pub fn state<'a>(&self, cx: &'a gpui::App) -> Option<&'a Bluetooth> {
        self.bluetooth.read(cx).get()
    }

    /// Shown or not: fresh each time it opens; novad looks for devices
    /// while it shows.
    fn follow_stage(&mut self, cx: &mut Context<Self>) {
        let screen = self.screen.id;
        let shown = Stage::is_open(QUICK_SETTINGS, screen, cx)
            && Stage::page(QUICK_SETTINGS, screen, cx).as_deref() == Some(PAGE);
        if shown == self.shown {
            return;
        }
        self.shown = shown;
        Watch::want(&self.watch, cx.entity_id(), shown, cx);
        if shown {
            self.opened = None;
            self.forgetting = None;
            self.closed.clear();
            self.hovered = None;
            self.focused_for = None;
        }
        cx.notify();
    }

    fn follow_bluetooth(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let on = is_on(self.state(cx));
        match self.shown {
            true if self.power_knob.target() != on => self.power_knob.set_target(on, now),
            true => {}
            // Out of sight it is just there.
            false => self.power_knob = Eased::lasting(on, DURATION_COLOR, now),
        }
        // What was typed goes with its question.
        let id = self.state(cx).and_then(|state| state.pairing.as_ref()).map(|request| request.id);
        if id != self.pin_for {
            self.pin_for = id;
            if id.is_none() {
                self.focused_for = None;
            }
            if !self.pin.read(cx).text().is_empty() {
                self.pin.update(cx, |field, cx| field.set_text("", cx));
            }
        }
        // A closed failure shows again once it's gone and comes back.
        if let Some(state) = self.bluetooth.read(cx).get() {
            let failing = |address: &String, error: &String| {
                state.devices.iter().any(|device| &device.address == address && device.error.as_ref() == Some(error))
            };
            self.closed.retain(|address, error| failing(address, error));
        }
        cx.notify();
    }

    /// A click on a device: connects a new or known one (pairing a new one
    /// first), disconnects a connected one.
    pub fn act(&mut self, device: &BluetoothDevice, cx: &mut Context<Self>) {
        self.forgetting = None;
        let address = device.address.clone();
        match logic::click(device.status) {
            Some(logic::Click::Connect) => {
                self.opened = None;
                Daemon::send(Command::BluetoothConnect { address }, cx);
            }
            Some(logic::Click::Disconnect) => {
                self.opened = None;
                Daemon::send(Command::BluetoothDisconnect { address }, cx);
            }
            None => {}
        }
        cx.notify();
    }

    /// A right click: the device's menu, or shut it when it's open.
    pub fn open_menu(&mut self, device: &BluetoothDevice, cx: &mut Context<Self>) {
        let address = Some(device.address.clone());
        match logic::menu(device.status) {
            Some(logic::Menu::Actions) => {
                self.forgetting = None;
                self.opened = if self.opened == address { None } else { address };
            }
            Some(logic::Menu::Forget) => {
                self.opened = None;
                self.forgetting = if self.forgetting == address { None } else { address };
            }
            None => {}
        }
        cx.notify();
    }

    /// A known device's ⋯: asks to forget it, or stops asking.
    pub fn toggle_forget(&mut self, address: &str, cx: &mut Context<Self>) {
        self.opened = None;
        self.forgetting = match self.forgetting.as_deref() == Some(address) {
            true => None,
            false => Some(address.to_owned()),
        };
        cx.notify();
    }

    pub fn disconnect(&mut self, address: String, cx: &mut Context<Self>) {
        self.opened = None;
        Daemon::send(Command::BluetoothDisconnect { address }, cx);
        cx.notify();
    }

    pub fn forget(&mut self, address: String, cx: &mut Context<Self>) {
        self.opened = None;
        self.forgetting = None;
        Daemon::send(Command::BluetoothForget { address }, cx);
        cx.notify();
    }

    pub fn close_failure(&mut self, device: &BluetoothDevice, cx: &mut Context<Self>) {
        if let Some(error) = &device.error {
            self.closed.insert(device.address.clone(), error.clone());
        }
        cx.notify();
    }

    pub fn hover(&mut self, address: &str, hovered: bool, cx: &mut Context<Self>) {
        let now = match hovered {
            true => Some(address.to_owned()),
            false if self.hovered.as_deref() == Some(address) => None,
            false => return,
        };
        if now != self.hovered {
            self.hovered = now;
            cx.notify();
        }
    }

    /// Enter answers a typed question; every other key is left alone, so
    /// Esc closes quick settings.
    pub fn key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "enter" {
            return;
        }
        let Some(request) = self.state(cx).and_then(|state| state.pairing.clone()) else { return };
        if logic::typed(&request.ask) {
            crate::prompt::accept(&request, &self.pin, cx);
            cx.stop_propagation();
        }
    }

    pub fn set_power(radio: BluetoothRadio, cx: &mut gpui::App) {
        match radio {
            BluetoothRadio::On => Daemon::send(Command::BluetoothPower { on: false }, cx),
            BluetoothRadio::Off => Daemon::send(Command::BluetoothPower { on: true }, cx),
            BluetoothRadio::Missing | BluetoothRadio::SwitchedOff => {}
        }
    }
}

fn is_on(state: Option<&Bluetooth>) -> f32 {
    if state.is_some_and(|state| state.radio == BluetoothRadio::On) { 1.0 } else { 0.0 }
}
