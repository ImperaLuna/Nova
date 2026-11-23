//! The pill: the shown tool's logo and its session percent (or its only
//! window's), in the theme's primary color from 80%. While novad is still
//! reading a tool a spinning arc sits beside the logo; a tool with no
//! numbers has a dashed ring there and a dim logo. Hidden until novad's
//! first event, as today's pill waits for it.

use crate::logo::logo;
use crate::panel::NAME;
use crate::usage::{Summary, shown, summarize};
use gpui::{
    Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled, Subscription, Window, div,
};
use jiff::Timestamp;
use jiff::tz::TimeZone;
use nova_ai_usage_api::{AiUsage, Event, TOPIC, UsageState};
use nova_look::tokens::{
    BAR_HEIGHT, DIM_OPACITY, FONT_MONO, FONT_SIZE, ICON_GAP, ICON_SIZE, ICON_SIZE_SMALL, PAD, WEIGHT_LABEL,
};
use nova_look::{Spinner, Theme};
use nova_stage::{Screen, Shown, Stage};
use nova_ui_client::Live;

pub struct UsagePill {
    screen: Screen,
    usage: Entity<Live<AiUsage>>,
    shown: Shown,
    _followed: Subscription,
}

impl UsagePill {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let usage = Live::follow(TOPIC, cx, |Event::AiUsage(usage)| Some(usage));
        let followed = cx.observe(&usage, |pill: &mut Self, usage, cx| {
            let has = usage.read(cx).get().is_some();
            pill.shown.set(has, cx);
            cx.notify();
        });
        Self { screen: screen.clone(), usage, shown: Shown::new(false), _followed: followed }
    }

    /// Whether the pill is shown, for the stage.
    pub fn shown(&self) -> Shown {
        self.shown.clone()
    }

    fn summary(&self, cx: &gpui::App) -> Summary {
        // The pill shows no times, so any zone will do.
        let usage = self.usage.read(cx).get();
        summarize(usage.and_then(shown), Timestamp::now(), &TimeZone::UTC)
    }
}

impl Render for UsagePill {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let summary = self.summary(cx);
        let unavailable = summary.state == UsageState::Unavailable;
        let loading = summary.state == UsageState::Loading;
        // Turning only while it can be seen: the open panel lies over it.
        if loading && self.shown.get() && !Stage::is_open(NAME, self.screen.id, cx) {
            window.request_animation_frame();
        }
        let ink = if unavailable { theme.fg.opacity(DIM_OPACITY) } else { theme.fg };
        let number = match (&summary.main, summary.warn()) {
            (None, _) => theme.fg_dim(),
            (Some(_), true) => theme.primary,
            (Some(_), false) => theme.fg,
        };
        let indicator = summary.has_indicator().then(|| {
            Spinner::new()
                .size(ICON_SIZE)
                .color(theme.fg_dim())
                .track(theme.track())
                .dashed(unavailable)
                .spinning(loading)
        });
        div()
            .id("ai-usage-pill")
            .h(BAR_HEIGHT)
            .px(PAD)
            .rounded_full()
            .cursor_pointer()
            .hover(|style| style.bg(theme.hover()))
            .flex()
            .items_center()
            .gap(ICON_GAP)
            .children(summary.provider.map(|provider| logo(provider, ICON_SIZE_SMALL, ink)))
            .children(indicator)
            .child(
                div()
                    .font_family(FONT_MONO)
                    .text_size(FONT_SIZE)
                    .font_weight(WEIGHT_LABEL)
                    .text_color(number)
                    .child(summary.hero_with_unit()),
            )
    }
}
