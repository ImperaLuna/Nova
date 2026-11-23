//! The panel the pill grows into, on one screen: the shown tool's name and
//! plan, its headline percent, a meter for each limit with how long is
//! left and when it resets, and at the bottom the other tools, a click on
//! one showing it in the bar. A click on the header closes it, as the pill
//! it grew from would.
//!
//! Opening asks novad for fresh numbers (it decides whether to fetch), and
//! the meters fill once the panel has grown. While it is open the "left"
//! and "resets" texts count down. It grows when a tool needs more room and
//! never shrinks back, so switching tools doesn't make it jump.

use crate::layout::{caption_line, panel_height};
use crate::logo::logo;
use crate::tokens::{COUNTDOWN, HERO_UNIT_SCALE};
use crate::usage::{Row, Summary, others, shown, summarize};
use gpui::{
    AnyElement, App, Context, Div, Entity, Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Pixels, Render, SharedString, Size, Styled, Subscription, Task, TextRun, Window, div, font, px,
    relative, size,
};
use jiff::Timestamp;
use jiff::tz::TimeZone;
use nova_ai_usage_api::{AiProvider, AiUsage, Command, Event, ProviderUsage, TOPIC, UsageState};
use nova_look::Theme;
use nova_look::motion::ease;
use nova_look::tokens::{
    BAR_HEIGHT, CHIP_HEIGHT, CHIP_PAD, DIM_OPACITY, DURATION_FADE, DURATION_FILL, DURATION_PANEL, FONT_MONO, FONT_SANS,
    FONT_SIZE_HERO, FONT_SIZE_SMALL, GAP, ICON_GAP, ICON_SIZE, ICON_SIZE_SMALL, LABEL_NUDGE, LINE_HEIGHT_HERO,
    LINE_HEIGHT_WRAPPED, METER_HEIGHT, METER_HEIGHT_SMALL, PAD, PANEL_PAD, PANEL_PAD_BOTTOM, PANEL_WIDTH, RADIUS_ROW,
    REVEAL_FILL, ROW_HEIGHT, ROW_INSET, SECTION_GAP, WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_stage::{Screen, Stage};
use nova_ui_client::{Daemon, Live};
use std::time::Instant;

/// The panel's name: the feature's id, which `nova toggle` uses.
pub const NAME: &str = "ai-usage";

/// What hovering another tool's row shows.
const ROW_GROUP: &str = "ai-usage-row";

pub struct UsagePanel {
    screen: Screen,
    usage: Entity<Live<AiUsage>>,
    zone: TimeZone,
    /// The time the texts count from: set as it opens and on each
    /// countdown.
    now: Timestamp,
    was_open: bool,
    /// When it last opened, for the meters' fill; `None` once it has faded
    /// out after closing.
    opened: Option<Instant>,
    countdown: Option<Task<()>>,
    /// The tallest it has been, and the size last handed to the stage.
    tallest: Pixels,
    asked: Size<Pixels>,
    _followed: Subscription,
}

impl UsagePanel {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let usage = Live::follow(TOPIC, cx, |Event::AiUsage(usage)| Some(usage));
        let followed = cx.observe(&usage, |_, _, cx| cx.notify());
        let mut panel = Self {
            screen: screen.clone(),
            usage,
            zone: TimeZone::system(),
            now: Timestamp::now(),
            was_open: false,
            opened: None,
            countdown: None,
            tallest: px(0.0),
            asked: size(PANEL_WIDTH, px(0.0)),
            _followed: followed,
        };
        // Before the first frame, a hint counts as one line.
        let height = panel.height(1, cx);
        panel.asked.height = height;
        panel
    }

    /// The size it asks the stage for.
    pub fn size(&self) -> Size<Pixels> {
        self.asked
    }

    fn data<'a>(&self, cx: &'a App) -> Option<&'a AiUsage> {
        self.usage.read(cx).get()
    }

    fn summary_of(&self, provider: Option<&ProviderUsage>) -> Summary {
        summarize(provider, self.now, &self.zone)
    }

    /// The height for what it shows now, taller only.
    fn height(&mut self, message_lines: usize, cx: &App) -> Pixels {
        let usage = self.data(cx);
        let summary = self.summary_of(usage.and_then(shown));
        let rows = usage.map_or(0, |usage| others(usage).count());
        self.tallest = self.tallest.max(panel_height(&summary, rows, message_lines));
        self.tallest
    }

    /// Called from `render` with whether the stage has the panel open.
    fn follow_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open == self.was_open {
            return;
        }
        self.was_open = open;
        eprintln!("INFO nova-ui: ai usage panel {}", if open { "opened" } else { "closed" });
        if !open {
            self.countdown = None;
            // Once it has faded out, the meters start empty again.
            cx.spawn(async move |panel, cx| {
                cx.background_executor().timer(DURATION_PANEL + DURATION_FADE).await;
                let _ = panel.update(cx, Self::faded);
            })
            .detach();
            return;
        }
        self.now = Timestamp::now();
        self.opened = Some(Instant::now());
        self.countdown = Some(cx.spawn(async move |panel, cx| {
            let mut open = true;
            while open {
                cx.background_executor().timer(COUNTDOWN).await;
                open = panel.update(cx, Self::tick).is_ok();
            }
        }));
        cx.defer_in(window, |_, _, cx| Daemon::send(Command::RefreshAiUsage, cx));
    }

    /// The panel has faded out after closing: unless it opened again, the
    /// meters start empty next time.
    fn faded(&mut self, cx: &mut Context<Self>) {
        if !self.was_open {
            self.opened = None;
            cx.notify();
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        self.now = Timestamp::now();
        cx.notify();
    }

    /// How full the meters are drawn (0..1): they fill once the panel has
    /// grown most of the way.
    fn fill(&self, now: Instant) -> f32 {
        let Some(opened) = self.opened else { return 0.0 };
        let waited = now.saturating_duration_since(opened).as_secs_f32() - DURATION_PANEL.as_secs_f32() * REVEAL_FILL;
        ease((waited / DURATION_FILL.as_secs_f32()).clamp(0.0, 1.0))
    }

    /// How many lines `message` wraps to across the panel.
    fn lines(message: &str, window: &Window) -> usize {
        if message.is_empty() {
            return 1;
        }
        let run = TextRun {
            len: message.len(),
            font: font(FONT_SANS),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_text(
            SharedString::from(message.to_owned()),
            FONT_SIZE_SMALL,
            &[run],
            Some(content_width()),
            None,
        );
        shaped.map_or(1, |lines| lines.iter().map(|line| line.wrap_boundaries().len() + 1).sum())
    }

    /// Follows the height into the stage, after this update: the stage
    /// reads this view to lay the panel out.
    fn ask_for(&mut self, height: Pixels, cx: &mut Context<Self>) {
        if height == self.asked.height {
            return;
        }
        self.asked.height = height;
        let (screen, asked) = (self.screen.id, self.asked);
        cx.defer(move |cx| Stage::resize(NAME, screen, asked, cx));
    }
}

/// The width inside the panel's padding.
fn content_width() -> Pixels {
    PANEL_WIDTH - PANEL_PAD * 2.0
}

/// A limit's percent in the theme's primary color from 80%.
fn percent_color(row: Option<&Row>, theme: &Theme) -> Hsla {
    match row {
        Some(row) if row.warn => theme.primary,
        Some(_) => theme.fg,
        None => theme.fg_dim(),
    }
}

/// Small dim text: the limits' names, times and the hint, shown as written.
fn caption(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div().text_size(FONT_SIZE_SMALL).text_color(theme.fg_dim()).child(text.into())
}

/// A meter: the empty track, filled to `row`'s percent of `fill`.
fn meter(row: &Row, height: Pixels, fill: f32, theme: &Theme) -> Div {
    let color = if row.warn { theme.primary } else { theme.meter() };
    let share = f32::from(row.percent) / 100.0 * fill;
    div()
        .h(height)
        .w_full()
        .rounded_full()
        .bg(theme.track())
        .child(div().h_full().w(relative(share)).rounded_full().bg(color))
}

impl Render for UsagePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.follow_open(Stage::is_open(NAME, self.screen.id, cx), window, cx);
        let now = Instant::now();
        let fill = self.fill(now);
        if self.opened.is_some() && fill < 1.0 {
            window.request_animation_frame();
        }
        let usage = self.data(cx).cloned();
        let summary = self.summary_of(usage.as_ref().and_then(shown));
        let height = self.height(Self::lines(&summary.message, window), cx);
        self.ask_for(height, cx);
        let theme = Theme::of(cx).clone();
        let rows: Vec<AnyElement> = usage
            .iter()
            .flat_map(others)
            .map(|provider| self.other(&self.summary_of(Some(provider)), &theme, cx))
            .collect();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.header(&summary, &theme, cx))
            .child(hero(&summary, &theme))
            .child(details(&summary, fill, &theme))
            .child(div().flex_1())
            .child(
                div()
                    .px(PANEL_PAD)
                    .pb(PANEL_PAD_BOTTOM)
                    .flex()
                    .flex_col()
                    .gap(GAP)
                    .child(div().h(px(1.0)).bg(theme.outline()))
                    .children(rows),
            )
    }
}

impl UsagePanel {
    /// The pill's place, grown into the header: the logo where the pill had
    /// it, the tool's name, its plan in a chip at the right.
    fn header(&self, summary: &Summary, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let ink = if summary.state == UsageState::Unavailable { theme.fg.opacity(DIM_OPACITY) } else { theme.fg };
        let plan = (!summary.plan.is_empty()).then(|| {
            div()
                .h(CHIP_HEIGHT)
                .px(CHIP_PAD)
                .rounded_full()
                .bg(theme.surface_raised())
                .flex()
                .items_center()
                .text_size(FONT_SIZE_SMALL)
                .font_weight(WEIGHT_LABEL)
                .text_color(theme.fg_dim())
                .child(summary.plan.clone())
        });
        let screen = self.screen.id;
        div()
            .h(BAR_HEIGHT)
            .flex_none()
            .pl(PAD)
            .pr(CHIP_PAD)
            .flex()
            .items_center()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, _: &MouseDownEvent, _, cx| Stage::close(NAME, screen, cx)),
            )
            .children(summary.provider.map(|provider| logo(provider, ICON_SIZE_SMALL, ink)))
            .child(div().flex_1().pl(ICON_GAP + LABEL_NUDGE).font_weight(WEIGHT_TITLE).truncate().child(summary.name))
            .children(plan)
    }

    /// Another tool: its logo, name and percent. A click shows it in the
    /// bar.
    fn other(&self, info: &Summary, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let provider: Option<AiProvider> = info.provider;
        let ink = if info.state == UsageState::Unavailable { theme.fg.opacity(DIM_OPACITY) } else { theme.fg };
        let hover = theme.hover();
        div()
            .group(ROW_GROUP)
            .h(ROW_HEIGHT)
            .mx(-ROW_INSET)
            .px(ROW_INSET)
            .rounded(RADIUS_ROW)
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .flex()
            .items_center()
            .gap(ROW_INSET)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                    if let Some(provider) = provider {
                        Daemon::send(Command::ShowAiUsage { provider }, cx);
                    }
                }),
            )
            .children(info.provider.map(|provider| logo(provider, ICON_SIZE, ink)))
            .child(div().flex_1().truncate().child(info.name))
            .child(
                div()
                    .font_family(FONT_MONO)
                    .font_weight(WEIGHT_LABEL)
                    .text_color(percent_color(info.main.as_ref(), theme))
                    .child(info.hero_with_unit()),
            )
            .into_any_element()
    }
}

/// The headline: the pill's number, big, with a smaller `%`.
fn hero(summary: &Summary, theme: &Theme) -> Div {
    let color = percent_color(summary.main.as_ref(), theme);
    let unit = summary.main.as_ref().map(|_| div().text_size(FONT_SIZE_HERO * HERO_UNIT_SCALE).child("%"));
    div()
        .mt(GAP)
        .h(LINE_HEIGHT_HERO)
        .flex_none()
        .px(PANEL_PAD)
        .flex()
        .items_baseline()
        .font_family(FONT_MONO)
        .font_weight(WEIGHT_LABEL)
        .text_color(color)
        .child(div().text_size(FONT_SIZE_HERO).child(summary.hero.clone()))
        .children(unit)
}

/// The limits: the bar's one with its meter (and the hint when there are
/// no numbers), then the second, smaller.
fn details(summary: &Summary, fill: f32, theme: &Theme) -> Div {
    let mut primary = div().flex().flex_col().gap(GAP);
    if let Some(main) = &summary.main {
        let left = summary.second.as_ref().map(|_| caption(main.left.clone(), theme).font_family(FONT_MONO));
        primary = primary
            .child(div().h(caption_line()).flex().justify_between().child(caption(main.name, theme)).children(left))
            .child(meter(main, METER_HEIGHT, fill, theme));
        // A lone window says when it resets under its meter.
        if summary.second.is_none() && !main.reset.is_empty() {
            primary = primary.child(caption(main.reset.clone(), theme));
        }
    }
    if !summary.message.is_empty() {
        primary = primary.child(caption(summary.message.clone(), theme).line_height(relative(LINE_HEIGHT_WRAPPED)));
    }
    let second = summary.second.as_ref().map(|second| {
        let reset = (!second.reset.is_empty()).then(|| caption(second.reset.clone(), theme));
        div()
            .flex()
            .flex_col()
            .gap(GAP)
            .child(
                div().flex().justify_between().items_baseline().child(second.name).child(
                    div()
                        .font_family(FONT_MONO)
                        .font_weight(WEIGHT_LABEL)
                        .text_color(percent_color(Some(second), theme))
                        .child(format!("{}%", second.percent)),
                ),
            )
            .child(meter(second, METER_HEIGHT_SMALL, fill, theme))
            .children(reset)
    });
    div().flex_none().px(PANEL_PAD).flex().flex_col().gap(SECTION_GAP).child(primary).children(second)
}
