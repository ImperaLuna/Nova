//! `--probe`: a plain window that shows what novad sends, to see the socket
//! client work before any feature is ported: the connection, novad's answer
//! to `status` (asked on every connection, so a restarted novad shows its
//! new pid), the feature list and the `sound` topic. It draws with fixed
//! colors, since the look kit isn't here yet, and goes once a ported
//! feature shows the same.

use gpui::{
    App, AppContext, Bounds, Context, Div, Entity, IntoElement, ParentElement, Render, SharedString, Styled,
    Subscription, TitlebarOptions, Window, WindowBounds, WindowOptions, div, px, rgb, size,
};
use nova_daemon_info::{Command, FeatureState, FeatureStatus, topics};
use nova_sound_api::{Device, Sound};
use nova_ui_client::{Connection, Daemon, Live, Status};

const WINDOW_WIDTH: f32 = 460.;
const WINDOW_HEIGHT: f32 = 620.;
const APP_ID: &str = "nova-ui-probe";
const TITLE: &str = "Nova probe";

// The probe's own colors, from the default theme's dark palette.
const BACKGROUND: u32 = 0x121110;
const SURFACE: u32 = 0x1a1918;
const TEXT: u32 = 0xf5e7c6;
const TEXT_DIM: u32 = 0xa8a296;
const GOOD: u32 = 0x25be6a;
const WAITING: u32 = 0xfaeb92;
const BAD: u32 = 0xee5396;

/// Opens the probe window. Without a compositor there is none to open.
pub fn open(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions { title: Some(TITLE.into()), ..Default::default() }),
        app_id: Some(APP_ID.into()),
        ..Default::default()
    };
    if let Err(e) = cx.open_window(options, |_, cx| cx.new(Probe::new)) {
        eprintln!("ERROR nova-ui: can't open the probe window: {e}");
    }
}

struct Probe {
    connection: Option<Entity<Connection>>,
    /// novad's answer to `status` on this connection.
    novad: Option<nova_daemon_info::Status>,
    features: Entity<Live<Vec<FeatureState>>>,
    sound: Entity<Live<Sound>>,
    _observed: Vec<Subscription>,
}

impl Probe {
    fn new(cx: &mut Context<Self>) -> Self {
        let connection = Daemon::connection(cx);
        let features = Live::follow(topics::FEATURES, cx, |event| match event {
            nova_daemon_info::Event::Features { features } => Some(features),
            _ => None,
        });
        let sound = Live::follow(nova_sound_api::TOPIC, cx, |nova_sound_api::Event::Sound(sound)| Some(sound));
        let mut observed =
            vec![cx.observe(&features, |_, _, cx| cx.notify()), cx.observe(&sound, |_, _, cx| cx.notify())];
        if let Some(connection) = &connection {
            observed.push(cx.observe(connection, Self::connection_changed));
        }
        Self { connection, novad: None, features, sound, _observed: observed }
    }

    /// Asks a novad that just connected who it is.
    fn connection_changed(&mut self, connection: Entity<Connection>, cx: &mut Context<Self>) {
        cx.notify();
        if !connection.read(cx).is_connected() {
            // Its pid says nothing about the novad that comes next.
            self.novad = None;
            return;
        }
        let status = Daemon::call::<nova_daemon_info::Status>(Command::Status, cx);
        cx.spawn(async move |this, cx| {
            let status = status.await;
            // The window may have closed meanwhile.
            let _ = this.update(cx, |probe, cx| {
                probe.novad = status.ok();
                cx.notify();
            });
        })
        .detach();
    }

    fn connection_row(&self, cx: &App) -> Div {
        let status = self.connection.as_ref().map(|connection| connection.read(cx).status().clone());
        let (color, text): (u32, SharedString) = match status {
            Some(Status::Connected { daemon }) => (GOOD, format!("novad {daemon}").into()),
            Some(Status::Lost) => (BAD, "novad went away".into()),
            Some(Status::Connecting) | None => (WAITING, "waiting for novad".into()),
        };
        let pid = self.novad.as_ref().map(|novad| format!("pid {}", novad.pid));
        row().child(dot(color)).child(div().flex_1().child(text)).children(pid.map(dim))
    }

    fn features_row(&self, cx: &App) -> Option<Div> {
        let features = self.features.read(cx).get()?;
        let ready = features.iter().filter(|feature| feature.status == FeatureStatus::Ready).count();
        Some(row().child(div().flex_1().child("Features")).child(dim(format!("{ready} of {}", features.len()))))
    }

    fn sound_section(&self, cx: &App) -> Option<Div> {
        let sound = self.sound.read(cx).get()?;
        let apps = sound.apps.iter().map(|app| level_row(app.name.clone(), app.volume, app.muted, false));
        let recorders = sound.recorders.iter().map(|name| row().child(name.clone()));
        let groups = [
            group("Outputs", sound.outputs.iter().map(device_row).collect()),
            group("Inputs", sound.inputs.iter().map(device_row).collect()),
            group("Apps", apps.collect()),
            group("Recording", recorders.collect()),
        ];
        Some(section().children(groups.into_iter().flatten()))
    }
}

impl Render for Probe {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(rgb(BACKGROUND))
            .text_color(rgb(TEXT))
            .text_sm()
            .child(section().child(self.connection_row(cx)).children(self.features_row(cx)))
            .children(self.sound_section(cx))
    }
}

fn device_row(device: &Device) -> Div {
    level_row(device.name.clone(), device.volume, device.muted, device.current)
}

/// A name with its volume: the one in use marked, a muted one dimmed.
fn level_row(name: String, volume: f32, muted: bool, current: bool) -> Div {
    let percent = format!("{}%", (volume * 100.).round());
    let mark = dot(if current { GOOD } else { SURFACE });
    let level = if muted { dim(percent).line_through() } else { div().child(percent) };
    row().child(mark).child(div().flex_1().child(name)).child(level)
}

fn section() -> Div {
    div().flex().flex_col().gap_3().p_3().rounded_lg().bg(rgb(SURFACE))
}

fn row() -> Div {
    div().flex().items_center().gap_2()
}

/// `rows` under their heading, or nothing without any.
fn group(heading: &'static str, rows: Vec<Div>) -> Option<Div> {
    (!rows.is_empty()).then(|| div().flex().flex_col().gap_1().child(dim(heading).text_xs()).children(rows))
}

fn dim(text: impl Into<SharedString>) -> Div {
    div().text_color(rgb(TEXT_DIM)).child(text.into())
}

fn dot(hex: u32) -> Div {
    div().size_2().rounded_full().bg(rgb(hex))
}
