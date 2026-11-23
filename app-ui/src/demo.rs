//! `--demo`: test pills and panels on the stage, to see it work with no
//! feature ported, to screenshot beside today's shell, and to count frames
//! (`--demo-toggles`, `--frames`). They are no feature's, so they come in
//! as slots of their own ([`slots`]) instead of from a manifest:
//!
//! - in the middle, a pill that grows into the `demo` panel on a click;
//! - on the left, a pill that counts seconds, which shows a view redrawing
//!   by itself while the stage asks for no frame;
//! - on the right, a pill that grows into `demo-right`, kept to the right
//!   edge. Opening one panel closes the other;
//! - beside it, a pill that grows into a stand-in for quick settings' panel, which draws
//!   the views of quick settings' hosts (`quick-settings.fill` and
//!   `quick-settings.pages`, the notifications' list and settings page)
//!   until quick settings is ported (IMP-406 removes it).

use gpui::{
    App, AppContext, Context, Div, IntoElement, ParentElement, Pixels, Render, SharedString, Styled, Subscription,
    Window, div, px, size,
};
use nova_daemon_info::Slot;
use nova_look::tokens::{
    FONT_MONO, FONT_SIZE_HEADING, FONT_SIZE_SMALL, GAP, ICON_GAP, ICON_SIZE_SMALL, PAD, PANEL_PAD, PANEL_WIDTH,
    RADIUS_PANEL, RADIUS_ROW, ROW_HEIGHT, ROW_INSET, SECTION_GAP, WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_look::{Glyph, Icon, LookStyled, Theme, Touching};
use nova_stage::{HOST_BAR_CENTER, HOST_BAR_LEFT, HOST_BAR_RIGHT, Panel, Parts, Pill, Screen, Stage};
use std::time::Duration;

const CENTER_PILL: &str = "demo.center";
const LEFT_PILL: &str = "demo.left";
const RIGHT_PILL: &str = "demo.right";
const QUICK_SETTINGS_PILL: &str = "demo.quick-settings";
/// The panels' names, as [`Stage::toggle`] takes them.
const CENTER_PANEL: &str = "demo";
const RIGHT_PANEL: &str = "demo-right";
/// Named as quick settings' panel, so `nova toggle quick-settings` opens it
/// and the pop-ups wait while it shows the list, as they will beside it.
const QUICK_SETTINGS_PANEL: &str = "quick-settings";
/// Quick settings' hosts the demo draws, as `ui/quick_settings` names them.
const QUICK_SETTINGS_HOSTS: [&str; 2] = ["quick-settings.fill", "quick-settings.pages"];

// The demo's own sizes and timings.
const CENTER_PANEL_WIDTH: Pixels = px(620.0);
const CENTER_PANEL_HEIGHT: Pixels = px(440.0);
const RIGHT_PANEL_HEIGHT: Pixels = px(260.0);
/// Each host's column as wide as today's quick settings' content.
const QUICK_SETTINGS_COLUMN: Pixels = px(336.0);
const QUICK_SETTINGS_HEIGHT: Pixels = px(480.0);
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
    nova_stage::pill(QUICK_SETTINGS_PILL, cx, |screen, cx| {
        let screen = screen.clone();
        let panel = cx.new(|cx| QuickSettingsHosts::new(&screen, cx));
        let width = QUICK_SETTINGS_COLUMN * QUICK_SETTINGS_HOSTS.len() as f32 + PANEL_PAD * 3.0;
        Pill::new(cx.new(|_| Label { glyph: Glyph::BELL, text: "Hosts".into() })).panel(Panel::new(
            QUICK_SETTINGS_PANEL,
            size(width, QUICK_SETTINGS_HEIGHT),
            panel,
        ))
    });
    [
        (HOST_BAR_CENTER, CENTER_PILL),
        (HOST_BAR_LEFT, LEFT_PILL),
        (HOST_BAR_RIGHT, QUICK_SETTINGS_PILL),
        (HOST_BAR_RIGHT, RIGHT_PILL),
    ]
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

/// Quick settings' hosts side by side, each drawing its views in slot
/// order, so a ported feature's parts can be seen before quick settings is.
struct QuickSettingsHosts {
    hosts: Vec<Parts>,
    _slots: Option<Subscription>,
}

impl QuickSettingsHosts {
    fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let hosts = QUICK_SETTINGS_HOSTS.iter().map(|host| Parts::new(host, screen, cx)).collect();
        let slots = Stage::slots(cx).map(|slots| {
            cx.observe(&slots, |hosts: &mut Self, _, cx| {
                let mut changed = false;
                for parts in &mut hosts.hosts {
                    changed |= parts.sync(cx);
                }
                if changed {
                    cx.notify();
                }
            })
        });
        Self { hosts, _slots: slots }
    }
}

impl Render for QuickSettingsHosts {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let columns = self.hosts.iter().map(|parts| {
            div().w(QUICK_SETTINGS_COLUMN).h_full().flex().flex_col().gap(SECTION_GAP).children(parts.views().cloned())
        });
        div().size_full().p(PANEL_PAD).flex().gap(PANEL_PAD).children(columns)
    }
}
