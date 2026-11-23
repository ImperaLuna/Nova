use gpui::{
    App, AppContext, Context, DisplayId, Div, InteractiveElement, IntoElement, ParentElement, Pixels, Render,
    SharedString, Styled, Window, div, prelude::FluentBuilder, px, size,
};
use nova_daemon_info::Slot;
use nova_look::tokens::{
    DIM_OPACITY, FONT_MONO, FONT_SIZE_HEADING, FONT_SIZE_SMALL, GAP, ICON_GAP, ICON_SIZE_SMALL, PAD, PANEL_PAD,
    PANEL_WIDTH, RADIUS_PANEL, RADIUS_ROW, ROW_HEIGHT, ROW_INSET, SECTION_GAP, WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_look::{Glyph, Icon, LookStyled, Theme, Touching};
use nova_stage::{HOST_BAR_CENTER, HOST_BAR_LEFT, HOST_BAR_RIGHT, Panel, Pill, Placement, Stage};
use std::time::Duration;

const CENTER_PILL: &str = "demo.center";
const LEFT_PILL: &str = "demo.left";
const RIGHT_PILL: &str = "demo.right";
const EDGE_PILL: &str = "demo.edge";
const WHOLE_PILL: &str = "demo.whole";
pub const CENTER_PANEL: &str = "demo";
const RIGHT_PANEL: &str = "demo-right";
const EDGE_PANEL: &str = "demo-edge";
const WHOLE_PANEL: &str = "demo-whole";

const CENTER_PANEL_WIDTH: Pixels = px(620.0);
const CENTER_PANEL_HEIGHT: Pixels = px(440.0);
const RIGHT_PANEL_HEIGHT: Pixels = px(260.0);
const EDGE_PANEL_WIDTH: Pixels = px(368.0);
const FOOTER_HEIGHT: Pixels = px(40.0);
const FOOTER_TINT: f32 = 0.9;
const TICK: Duration = Duration::from_secs(1);
const TOGGLES_START: Duration = Duration::from_secs(2);
const TOGGLE_EVERY: Duration = Duration::from_millis(700);

pub fn slots(cx: &mut App) -> Vec<Slot> {
    nova_stage::pill(CENTER_PILL, cx, |_, cx| {
        let panel = cx.new(|_| DemoPanel { title: "Demo panel", rows: &CENTER_ROWS, at_edge: false });
        Pill::new(cx.new(|_| Label { glyph: Glyph::SEARCH, text: "Demo".into() })).panel(Panel::new(
            CENTER_PANEL,
            size(CENTER_PANEL_WIDTH, CENTER_PANEL_HEIGHT),
            panel,
        ))
    });
    nova_stage::pill(LEFT_PILL, cx, |_, cx| Pill::new(cx.new(Seconds::start)));
    nova_stage::pill(RIGHT_PILL, cx, |_, cx| {
        let panel = cx.new(|_| DemoPanel { title: "Right panel", rows: &RIGHT_ROWS, at_edge: false });
        Pill::new(cx.new(|_| Label { glyph: Glyph::WIFI, text: "Right".into() })).panel(Panel::new(
            RIGHT_PANEL,
            size(PANEL_WIDTH, RIGHT_PANEL_HEIGHT),
            panel,
        ))
    });
    nova_stage::pill(EDGE_PILL, cx, |screen, cx| {
        let drawer = cx.new(|_| DemoPanel { title: "Edge drawer", rows: &CENTER_ROWS, at_edge: true });
        let drawer = Panel::new(EDGE_PANEL, size(EDGE_PANEL_WIDTH, screen.size.height), drawer);
        Pill::new(cx.new(|_| Label { glyph: Glyph::BLUETOOTH, text: "Edge".into() }))
            .panel(drawer.placement(Placement::RightEdge))
    });
    nova_stage::pill(WHOLE_PILL, cx, |screen, cx| {
        let over = cx.new(|_| OverScreen { screen: screen.id });
        Pill::new(cx.new(|_| Label { glyph: Glyph::HEADPHONES, text: "Whole".into() }))
            .panel(Panel::new(WHOLE_PANEL, screen.size, over).placement(Placement::WholeScreen))
    });
    [
        (HOST_BAR_CENTER, CENTER_PILL, 0),
        (HOST_BAR_LEFT, LEFT_PILL, 0),
        (HOST_BAR_LEFT, WHOLE_PILL, 0),
        (HOST_BAR_RIGHT, RIGHT_PILL, 0),
        (HOST_BAR_RIGHT, EDGE_PILL, 1),
    ]
    .map(|(host, view, order)| Slot { host: host.into(), view: view.into(), order })
    .into()
}

pub fn toggle_by_itself(panel: String, times: u32, cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(TOGGLES_START).await;
        for round in 0..times {
            let turn = (round / 2) as usize;
            cx.update(|cx| {
                let screens = Stage::screens(cx).map(|screens| screens.read(cx).list().to_vec()).unwrap_or_default();
                if let Some(screen) = screens.get(turn % screens.len().max(1)) {
                    Stage::toggle(&panel, screen.id, cx);
                }
            });
            cx.background_executor().timer(TOGGLE_EVERY).await;
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}

struct Label {
    glyph: Glyph,
    text: SharedString,
}

impl Render for Label {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        pill_content().child(Icon::new(self.glyph).size(ICON_SIZE_SMALL)).child(self.text.clone())
    }
}

struct Seconds {
    passed: u64,
}

impl Seconds {
    fn start(cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |seconds, cx| {
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

struct DemoPanel {
    title: &'static str,
    rows: &'static [(Glyph, &'static str, &'static str)],
    at_edge: bool,
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
                    .map(|footer| match self.at_edge {
                        true => footer.rounded_bl(RADIUS_PANEL),
                        false => footer.fit_corners(RADIUS_PANEL, px(0.0), Touching::Bottom),
                    })
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

struct OverScreen {
    screen: DisplayId,
}

impl Render for OverScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let screen = self.screen;
        let card = div()
            .w(PANEL_WIDTH)
            .p(PANEL_PAD)
            .rounded(RADIUS_PANEL)
            .bg(theme.surface())
            .flex()
            .flex_col()
            .gap(GAP)
            .child(div().text_size(FONT_SIZE_HEADING).font_weight(WEIGHT_TITLE).child("Over the whole screen"))
            .child(div().text_size(FONT_SIZE_SMALL).text_color(theme.fg_dim()).child("No glass, no blur"));
        div()
            .size_full()
            .bg(theme.backdrop.alpha(DIM_OPACITY))
            .flex()
            .items_center()
            .justify_center()
            .on_any_mouse_down(move |_, _, cx| Stage::close(WHOLE_PANEL, screen, cx))
            .child(card)
    }
}
