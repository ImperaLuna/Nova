//! `peripherals.card`: the wireless mice, keyboards, headsets and controllers
//! connected now, each with how it connects and its battery. novad leaves out
//! whatever isn't connected, and the card is shown only while it lists one. A
//! device that comes fades in, and a battery that changes eases to its new
//! level. A few rows, then it scrolls, so many devices never push the cards
//! below off the drawer.

use crate::logic::{self, Tone};
use crate::tokens::{
    DEVICE_CHIP_GAP, DEVICE_CHIP_HEIGHT, DEVICE_CHIP_PAD, DEVICE_ICON_SIZE, DEVICE_LEVEL_WIDTH, DEVICE_LINK_ICON_SIZE,
    DEVICE_LIST_MAX_ROWS, DEVICE_ROW_HEIGHT,
};
use gpui::{
    AnyElement, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement,
    Styled, Subscription, Window, div,
};
use nova_look::motion::Eased;
use nova_look::tokens::{
    DURATION_FILL, DURATION_PANEL, FONT_SIZE, FONT_SIZE_MICRO, FONT_SIZE_SMALL, GAP, MENU_PAD, RADIUS_CARD,
};
use nova_look::{CardTitle, DeviceBattery, Icon, Theme};
use nova_peripherals_api::{Event, Peripheral, Peripherals, TOPIC};
use nova_stage::Shown;
use nova_ui_client::Live;
use std::time::Instant;

/// A row's own motion: when its device came, and its battery's level.
struct Row {
    id: String,
    came: Instant,
    fill: Eased,
}

pub struct Card {
    peripherals: Entity<Live<Peripherals>>,
    shown: Shown,
    rows: Vec<Row>,
    _followed: Subscription,
}

impl Card {
    pub fn new(shown: Shown, cx: &mut Context<Self>) -> Self {
        let peripherals = Live::follow(TOPIC, cx, |Event::Peripherals(peripherals)| Some(peripherals));
        let followed = cx.observe(&peripherals, |card, _, cx| card.follow(cx));
        let mut card = Self { peripherals, shown, rows: Vec::new(), _followed: followed };
        card.follow(cx);
        card
    }

    /// A new list: keeps the rows whose device stays, easing a battery that
    /// changed, and fades in the new ones.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        let devices = self.devices(cx).to_vec();
        let mut rows = Vec::with_capacity(devices.len());
        for device in &devices {
            let fill = logic::fill(device.battery);
            let row = match self.rows.iter().position(|row| row.id == device.id) {
                Some(at) => {
                    let mut row = self.rows.swap_remove(at);
                    row.fill.set_target(fill, now);
                    row
                }
                None => Row { id: device.id.clone(), came: now, fill: Eased::lasting(fill, DURATION_FILL, now) },
            };
            rows.push(row);
        }
        self.rows = rows;
        self.shown.set(!devices.is_empty(), cx);
        cx.notify();
    }

    fn devices<'a>(&self, cx: &'a gpui::App) -> &'a [Peripheral] {
        self.peripherals.read(cx).get().map_or(&[], |peripherals| peripherals.devices.as_slice())
    }
}

impl Render for Card {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let fading = |row: &Row| now.saturating_duration_since(row.came) < DURATION_PANEL;
        if self.rows.iter().any(|row| fading(row) || !row.fill.settled(now)) {
            window.request_animation_frame();
        }
        let theme = Theme::of(cx);
        let devices = self.devices(cx).to_vec();
        let rows: Vec<AnyElement> = devices
            .iter()
            .zip(&self.rows)
            .map(|(device, row)| {
                let came = now.saturating_duration_since(row.came).as_secs_f32() / DURATION_PANEL.as_secs_f32();
                device_row(device, row.fill.value(now), came.min(1.0), cx)
            })
            .collect();
        div().w_full().flex().flex_col().gap(GAP).child(CardTitle::new("Devices")).child(
            div().p(MENU_PAD).rounded(RADIUS_CARD).bg(theme.surface_raised()).child(
                div()
                    .id("devices")
                    .max_h(DEVICE_ROW_HEIGHT * DEVICE_LIST_MAX_ROWS as f32)
                    .overflow_y_scroll()
                    .children(rows),
            ),
        )
    }
}

/// One device: its icon and name, how it connects, its battery and the
/// level beside it.
fn device_row(device: &Peripheral, fill: f32, opacity: f32, cx: &gpui::App) -> AnyElement {
    let theme = Theme::of(cx);
    let low = logic::low(device.battery, device.charging);
    let (amount, tone) = logic::amount(device.battery, device.charging);
    let ink = match tone {
        Tone::Plain => theme.fg,
        Tone::Soft => theme.fg_soft(),
        Tone::Low => theme.error,
    };
    let chip = device.link.map(|link| {
        div()
            .flex_none()
            .h(DEVICE_CHIP_HEIGHT)
            .px(DEVICE_CHIP_PAD)
            .rounded_full()
            .bg(theme.track())
            .flex()
            .items_center()
            .gap(DEVICE_CHIP_GAP)
            .text_size(FONT_SIZE_MICRO)
            .text_color(theme.fg_soft())
            .child(Icon::new(logic::link_icon(link)).size(DEVICE_LINK_ICON_SIZE).color(theme.fg_soft()))
            .child(logic::link_label(link))
    });
    div()
        .h(DEVICE_ROW_HEIGHT)
        .px(GAP)
        .flex()
        .items_center()
        .gap(GAP)
        .opacity(opacity)
        .child(Icon::new(logic::icon(device.kind)).size(DEVICE_ICON_SIZE).color(theme.fg))
        .child(div().flex_1().min_w_0().truncate().text_size(FONT_SIZE).text_color(theme.fg).child(device.name.clone()))
        .children(chip)
        .children(device.battery.map(|_| DeviceBattery::new(fill).low(low).charging(device.charging)))
        .child(
            div()
                .flex_none()
                .w(DEVICE_LEVEL_WIDTH)
                .flex()
                .justify_end()
                .text_size(FONT_SIZE_SMALL)
                .text_color(ink)
                .child(amount),
        )
        .into_any_element()
}
