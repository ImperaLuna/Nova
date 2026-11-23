use crate::{PAGE, QUICK_SETTINGS, TRAFFIC, logic, watch_traffic};
use gpui::{AppContext, Context, Entity, FocusHandle, KeyDownEvent, Subscription, Window};
use nova_look::motion::Eased;
use nova_look::tokens::DURATION_COLOR;
use nova_look::{FieldEvent, TextField};
use nova_network_api::{
    AttemptOutcome, Command, Event, LinkKind, Network, NetworkTraffic, TOPIC, WifiNetwork, WifiPassword,
};
use nova_stage::{Screen, Stage};
use nova_ui_client::{Daemon, Live, Watch};
use std::time::Instant;

pub struct NetworkPage {
    pub screen: Screen,
    pub network: Entity<Live<Network>>,
    pub traffic: Entity<Live<NetworkTraffic>>,
    traffic_watch: Entity<Watch>,
    shown: bool,
    pub asked: Option<String>,
    pub password: Entity<TextField>,
    pub wrong: bool,
    pub show_password: bool,
    pub menu_for: Option<String>,
    handled: Option<(String, AttemptOutcome)>,
    pub focus_password: bool,
    pub wifi_knob: Eased,
    _followed: Vec<Subscription>,
}

impl NetworkPage {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let network = Live::follow(TOPIC, cx, |event| match event {
            Event::Network(network) => Some(network),
            Event::NetworkTraffic(_) => None,
        });
        let traffic = Live::follow(TOPIC, cx, |event| match event {
            Event::NetworkTraffic(traffic) => Some(traffic),
            Event::Network(_) => None,
        });
        let password = cx.new(TextField::new);
        password.update(cx, |field, cx| field.set_masked(true, cx));
        let followed = vec![
            cx.observe(&network, |page, _, cx| page.follow_network(cx)),
            cx.observe(&traffic, |_, _, cx| cx.notify()),
            cx.subscribe(&password, |page, _, FieldEvent::Changed, cx| {
                page.wrong = false;
                cx.notify();
            }),
        ];
        let mut followed = followed;
        followed.extend(Stage::observe_panels(cx, |page, cx| page.follow_stage(cx)));
        let traffic_watch = Watch::named(TRAFFIC, watch_traffic, cx);
        let me = cx.entity_id();
        cx.on_release({
            let watch = traffic_watch.clone();
            move |_, cx| Watch::want(&watch, me, false, cx)
        })
        .detach();
        let on = logic::wifi_on(network.read(cx).get());
        Self {
            screen: screen.clone(),
            network,
            traffic,
            traffic_watch,
            shown: false,
            asked: None,
            password,
            wrong: false,
            show_password: false,
            menu_for: None,
            handled: None,
            focus_password: false,
            wifi_knob: Eased::lasting(if on { 1.0 } else { 0.0 }, DURATION_COLOR, Instant::now()),
            _followed: followed,
        }
    }

    pub fn network<'a>(&self, cx: &'a gpui::App) -> Option<&'a Network> {
        self.network.read(cx).get()
    }

    fn follow_stage(&mut self, cx: &mut Context<Self>) {
        let screen = self.screen.id;
        let shown = Stage::is_open(QUICK_SETTINGS, screen, cx)
            && Stage::page(QUICK_SETTINGS, screen, cx).as_deref() == Some(PAGE);
        if shown == self.shown {
            return;
        }
        self.shown = shown;
        Watch::want(&self.traffic_watch, cx.entity_id(), shown, cx);
        if shown {
            self.asked = None;
            self.wrong = false;
            self.show_password = false;
            self.menu_for = None;
            self.password.update(cx, |field, cx| {
                field.set_text("", cx);
                field.set_masked(true, cx);
            });
            self.handled = attempt(self.network(cx));
            Daemon::send(Command::ScanWifi, cx);
        }
        cx.notify();
    }

    fn follow_network(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let on = if logic::wifi_on(self.network(cx)) { 1.0 } else { 0.0 };
        match self.shown {
            true if self.wifi_knob.target() != on => self.wifi_knob.set_target(on, now),
            true => {}
            false => self.wifi_knob = Eased::lasting(on, DURATION_COLOR, now),
        }
        let outcome = attempt(self.network(cx));
        if self.shown && outcome != self.handled {
            self.handled = outcome.clone();
            match outcome {
                Some((ssid, AttemptOutcome::WrongPassword)) => {
                    let placeholder = placeholder(&ssid);
                    self.password.update(cx, |field, cx| field.set_placeholder(placeholder, cx));
                    self.asked = Some(ssid);
                    self.wrong = true;
                    self.focus_password = true;
                }
                Some((_, AttemptOutcome::Connected)) => {
                    self.password.update(cx, |field, cx| field.set_text("", cx));
                }
                _ => {}
            }
        }
        cx.notify();
    }

    pub fn pick(&mut self, net: &WifiNetwork, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_for = None;
        let busy = logic::connecting(self.network(cx)).is_some();
        if !logic::joinable(net) || net.connected || busy {
            cx.notify();
            return;
        }
        if !logic::needs_password(net) {
            connect(&net.ssid, None, cx);
            cx.notify();
            return;
        }
        if self.asked.as_deref() == Some(net.ssid.as_str()) {
            self.cancel(window, cx);
            return;
        }
        self.asked = Some(net.ssid.clone());
        self.wrong = false;
        let placeholder = placeholder(&net.ssid);
        self.password.update(cx, |field, cx| {
            field.set_text("", cx);
            field.set_placeholder(placeholder, cx);
        });
        window.focus(&self.password_focus(cx), cx);
        cx.notify();
    }

    pub fn password_focus(&self, cx: &gpui::App) -> FocusHandle {
        self.password.read(cx).focus_handle()
    }

    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = self.password.read(cx).text().to_owned();
        let Some(ssid) = self.asked.clone().filter(|_| logic::password_fits(&password)) else { return };
        self.asked = None;
        self.wrong = false;
        window.blur(cx);
        connect(&ssid, Some(WifiPassword::new(password)), cx);
        cx.notify();
    }

    pub fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.asked = None;
        self.wrong = false;
        self.password.update(cx, |field, cx| field.set_text("", cx));
        window.blur(cx);
        cx.notify();
    }

    pub fn toggle_shown(&mut self, cx: &mut Context<Self>) {
        self.show_password = !self.show_password;
        let masked = !self.show_password;
        self.password.update(cx, |field, cx| field.set_masked(masked, cx));
        cx.notify();
    }

    pub fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.asked.is_none() {
            return;
        }
        match event.keystroke.key.as_str() {
            "enter" => self.submit(window, cx),
            "escape" => self.cancel(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    pub fn toggle_menu(&mut self, ssid: &str, cx: &mut Context<Self>) {
        self.menu_for = match self.menu_for.as_deref() == Some(ssid) {
            true => None,
            false => Some(ssid.to_owned()),
        };
        cx.notify();
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu_for.take().is_some() {
            cx.notify();
        }
    }

    pub fn set_wifi(enabled: bool, blocked: bool, cx: &mut gpui::App) {
        if !blocked {
            Daemon::send(Command::SetWifi { enabled: !enabled }, cx);
        }
    }

    pub fn disconnect(&mut self, cx: &mut Context<Self>) {
        self.menu_for = None;
        Daemon::send(Command::DisconnectWifi, cx);
        cx.notify();
    }

    pub fn forget(&mut self, ssid: String, cx: &mut Context<Self>) {
        self.menu_for = None;
        Daemon::send(Command::ForgetWifi { ssid }, cx);
        cx.notify();
    }

    pub fn prefer(kind: LinkKind, cx: &mut gpui::App) {
        Daemon::send(Command::PreferNetwork { kind }, cx);
    }
}

fn placeholder(ssid: &str) -> String {
    format!("Password for {ssid}")
}

fn attempt(network: Option<&Network>) -> Option<(String, AttemptOutcome)> {
    let attempt = network?.wifi.as_ref()?.attempt.as_ref()?;
    Some((attempt.ssid.clone(), attempt.outcome))
}

fn connect(ssid: &str, password: Option<WifiPassword>, cx: &mut gpui::App) {
    Daemon::send(Command::ConnectWifi { ssid: ssid.to_owned(), password }, cx);
}
