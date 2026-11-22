//! `--demo`: test pills and panels on the stage, to see it work with no
//! feature ported, to screenshot beside today's shell, and to count frames
//! (`--demo-toggles`, `--frames`). They are no feature's, so they come in
//! as slots of their own ([`slots`]) instead of from a manifest:
//!
//! - in the middle, a pill that grows into the `demo` panel on a click;
//! - on the left, a pill that counts seconds, which shows a view redrawing
//!   by itself while the stage asks for no frame;
//! - on the right, a pill that grows into `demo-right`, kept to the right
//!   edge. Opening one panel closes the other.

use gpui::{
    App, AppContext, Context, Div, IntoElement, ParentElement, Pixels, Render, SharedString, Styled, Window, div, px,
    size,
};
use nova_daemon_info::Slot;
use nova_look::tokens::{
    FONT_MONO, FONT_SIZE_HEADING, FONT_SIZE_SMALL, GAP, ICON_GAP, ICON_SIZE_SMALL, PAD, PANEL_PAD, PANEL_WIDTH,
    RADIUS_PANEL, RADIUS_ROW, ROW_HEIGHT, ROW_INSET, SECTION_GAP, WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_look::{Glyph, Icon, LookStyled, Theme, Touching};
use nova_stage::{HOST_BAR_CENTER, HOST_BAR_LEFT, HOST_BAR_RIGHT, Panel, Pill, Stage};
use std::time::Duration;

const CENTER_PILL: &str = "demo.center";
const LEFT_PILL: &str = "demo.left";
const RIGHT_PILL: &str = "demo.right";
/// The panels' names, as [`Stage::toggle`] takes them.
const CENTER_PANEL: &str = "demo";
const RIGHT_PANEL: &str = "demo-right";

// The demo's own sizes and timings.
const CENTER_PANEL_WIDTH: Pixels = px(620.0);
const CENTER_PANEL_HEIGHT: Pixels = px(440.0);
const RIGHT_PANEL_HEIGHT: Pixels = px(260.0);
const FOOTER_HEIGHT: Pixels = px(40.0);
const FOOTER_TINT: f32 = 0.9;
const TICK: Duration = Duration::from_secs(1);
/// Before the first toggle, for the first frame and the GPU to settle; and
/// between two toggles, longer than a morph.
const TOGGLES_START: Duration = Duration::from_secs(2);
const TOGGLE_EVERY: Duration = Duration::from_millis(700);

/// Registers the demo's views and returns the slots that show them.
pub fn slots(cx: &mut App) -> Vec<Slot> {
    nova_stage::pill(CENTER_PILL, cx, |_, cx| {
        let panel = cx.new(|_| DemoPanel { title: "Demo panel", rows: &CENTER_ROWS });
        Pill::new(cx.new(|_| Label { glyph: Glyph::SEARCH, text: "Demo".into() })).panel(Panel::new(
            CENTER_PANEL,
            size(CENTER_PANEL_WIDTH, CENTER_PANEL_HEIGHT),
            panel,
        ))
    });
    nova_stage::pill(LEFT_PILL, cx, |_, cx| Pill::new(cx.new(Seconds::start)));
    nova_stage::pill(RIGHT_PILL, cx, |_, cx| {
        let panel = cx.new(|_| DemoPanel { title: "Right panel", rows: &RIGHT_ROWS });
        Pill::new(cx.new(|_| Label { glyph: Glyph::WIFI, text: "Right".into() })).panel(Panel::new(
            RIGHT_PANEL,
            size(PANEL_WIDTH, RIGHT_PANEL_HEIGHT),
            panel,
        ))
    });
    [(HOST_BAR_CENTER, CENTER_PILL), (HOST_BAR_LEFT, LEFT_PILL), (HOST_BAR_RIGHT, RIGHT_PILL)]
        .map(|(host, view)| Slot { host: host.into(), view: view.into(), order: 0 })
        .into()
}

/// Opens and closes the `demo` panel `times` times, one screen after the
/// other, then quits: with `--frames`, each motion's line says how many of
/// its frames came late.
pub fn toggle_by_itself(times: u32, cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(TOGGLES_START).await;
        for round in 0..times {
            // Open and close on one screen, then the next.
            let turn = (round / 2) as usize;
            cx.update(|cx| {
                let screens = Stage::screens(cx).map(|screens| screens.read(cx).list().to_vec()).unwrap_or_default();
                if let Some(screen) = screens.get(turn % screens.len().max(1)) {
                    Stage::toggle(CENTER_PANEL, screen.id, cx);
                }
            });
            cx.background_executor().timer(TOGGLE_EVERY).await;
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}

/// A pill's content: an icon and a word.
struct Label {
    glyph: Glyph,
    text: SharedString,
}

impl Render for Label {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        pill_content().child(Icon::new(self.glyph).size(ICON_SIZE_SMALL)).child(self.text.clone())
    }
}

/// A pill's content that counts the seconds since it was built.
struct Seconds {
    passed: u64,
}

impl Seconds {
    fn start(cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |seconds, cx| {
            // Until its pill is gone.
            let mut shown = true;
            while shown {
                cx.background_executor().timer(TICK).await;
                shown = seconds.update(cx, Self::tick).is_ok();
            }
        })
        .detach();
        Self { passed: 0 }
    }
}

impl Seconds {
    fn tick(&mut self, cx: &mut Context<Self>) {
        self.passed += 1;
        cx.notify();
    }
}

impl Render for Seconds {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        pill_content().font_family(FONT_MONO).font_weight(WEIGHT_LABEL).child(format!("{} s", self.passed))
    }
}

fn pill_content() -> Div {
    div().h_full().px(PAD).flex().items_center().gap(ICON_GAP)
}

const CENTER_ROWS: [(Glyph, &str, &str); 4] = [
    (Glyph::WIFI, "Wi-Fi", "Home"),
    (Glyph::BLUETOOTH, "Bluetooth", "2 devices"),
    (Glyph::HEADPHONES, "Sound", "Headphones"),
    (Glyph::MIC_OFF, "Microphone", "Off"),
];
const RIGHT_ROWS: [(Glyph, &str, &str); 2] = [(Glyph::WIFI, "Wi-Fi", "Home"), (Glyph::BELL, "Notifications", "3")];

/// A panel's content: a title, rows, and a footer that reaches the bottom
/// corners.
struct DemoPanel {
    title: &'static str,
    rows: &'static [(Glyph, &'static str, &'static str)],
}

impl Render for DemoPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let rows = self.rows.iter().map(|(glyph, label, detail)| {
            div()
                .h(ROW_HEIGHT)
                .px(ROW_INSET)
                .rounded(RADIUS_ROW)
                .flex()
                .items_center()
                .gap(GAP)
                .child(Icon::new(*glyph))
                .child(div().flex_1().child(*label))
                .child(div().text_size(FONT_SIZE_SMALL).text_color(theme.fg_dim()).child(*detail))
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SECTION_GAP)
                    .p(PANEL_PAD)
                    .child(div().text_size(FONT_SIZE_HEADING).font_weight(WEIGHT_TITLE).child(self.title))
                    .child(div().flex().flex_col().children(rows)),
            )
            .child(div().flex_1())
            .child(
                div()
                    .h(FOOTER_HEIGHT)
                    .fit_corners(RADIUS_PANEL, px(0.0), Touching::Bottom)
                    .bg(theme.primary.alpha(FOOTER_TINT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(ICON_GAP)
                    .text_color(theme.on_accent)
                    .font_weight(WEIGHT_TITLE)
                    .child(Icon::new(Glyph::CHECK).color(theme.on_accent))
                    .child("Apply"),
            )
    }
}
