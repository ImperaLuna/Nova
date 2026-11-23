//! `resources.card`: CPU, memory, GPU and the CPU's temperature as rings at
//! the drawer's foot, a dash for a reading novad doesn't have. A click
//! opens the resources page. The rings fill once the drawer has grown most
//! of the way, and empty with it as it closes. `ResourcesCard.qml` in
//! today's shell.
//!
//! The card is drawn whenever the drawer is, so it also decides when this
//! screen watches the machine: while the drawer is open on its first page
//! or on the resources page.

use crate::logic;
use crate::ring::Ring;
use crate::{PAGE, QUICK_SETTINGS};
use gpui::{
    Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled,
    Subscription, Window, div,
};
use nova_look::motion::ease;
use nova_look::tokens::{DURATION_FILL, DURATION_PANEL, GAP, ICON_SIZE_SMALL, PAD, RADIUS_CARD, REVEAL_FILL};
use nova_look::{CardTitle, Glyph, Icon, Theme};
use nova_resources_api::{Command, Event, Resources, TOPIC};
use nova_stage::{Screen, Stage};
use nova_ui_client::{Live, Watch};
use std::time::Instant;

pub struct Card {
    screen: Screen,
    resources: Entity<Live<Resources>>,
    /// The drawer is open on this screen, and since when; or closed, and
    /// since when, with how full the rings were then.
    open: bool,
    since: Instant,
    fill_at_close: f32,
    /// novad samples the machine while some screen's card wants it.
    watch: Entity<Watch>,
    /// Whether this screen wants it, as last told.
    watching: bool,
    _followed: Vec<Subscription>,
}

impl Card {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let resources = Live::follow(TOPIC, cx, |Event::Resources(resources)| Some(resources));
        let mut followed = vec![cx.observe(&resources, |_, _, cx| cx.notify())];
        followed.extend(Stage::observe_panels(cx, |card, cx| card.follow_stage(cx)));
        let watch = Watch::named("resources", |watching| Command::WatchResources { watching }, cx);
        // Gone with its screen or its feature: this screen wants it no more.
        let me = cx.entity_id();
        followed.push(cx.on_release(move |card: &mut Self, cx| {
            if card.watching {
                Watch::want(&card.watch, me, false, cx);
            }
        }));
        let mut card = Self {
            screen: screen.clone(),
            resources,
            open: false,
            since: Instant::now(),
            fill_at_close: 0.0,
            watch,
            watching: false,
            _followed: followed,
        };
        card.follow_stage(cx);
        card
    }

    fn follow_stage(&mut self, cx: &mut Context<Self>) {
        let screen = self.screen.id;
        let open = Stage::is_open(QUICK_SETTINGS, screen, cx);
        let page = Stage::page(QUICK_SETTINGS, screen, cx);
        let watching = open && page.as_deref().is_none_or(|page| page == PAGE);
        if watching != self.watching {
            self.watching = watching;
            let me = cx.entity_id();
            Watch::want(&self.watch, me, watching, cx);
        }
        if open != self.open {
            let now = Instant::now();
            if !open {
                self.fill_at_close = self.fill(now);
            }
            self.open = open;
            self.since = now;
            cx.notify();
        }
    }

    /// How full the rings are drawn, 0 to 1.
    fn fill(&self, now: Instant) -> f32 {
        let since = now.saturating_duration_since(self.since).as_secs_f32();
        match self.open {
            true => {
                let waited = since - DURATION_PANEL.as_secs_f32() * REVEAL_FILL;
                ease((waited / DURATION_FILL.as_secs_f32()).clamp(0.0, 1.0))
            }
            false => self.fill_at_close * (1.0 - ease((since / DURATION_PANEL.as_secs_f32()).min(1.0))),
        }
    }
}

impl Render for Card {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let fill = self.fill(now);
        let settled = match self.open {
            true => fill >= 1.0,
            false => fill <= 0.0,
        };
        if !settled {
            window.request_animation_frame();
        }
        let theme = Theme::of(cx);
        let hover = theme.hover();
        let rings = logic::rings(self.resources.read(cx).get());
        let screen = self.screen.id;
        div()
            .id("resources")
            .w_full()
            .p(PAD)
            .rounded(RADIUS_CARD)
            .bg(theme.surface_raised())
            .hover(move |style| style.bg(hover))
            .cursor_pointer()
            .on_click(move |_, _, cx| Stage::show_page(QUICK_SETTINGS, screen, Some(PAGE), cx))
            .flex()
            .flex_col()
            .gap(GAP)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(CardTitle::new("Resources"))
                    .child(Icon::new(Glyph::RIGHT).size(ICON_SIZE_SMALL).color(theme.fg_soft())),
            )
            .child(div().flex().children(rings.into_iter().map(|ring| {
                div().flex_1().flex().justify_center().child(Ring::new(ring.value * fill, ring.reading, ring.label))
            })))
    }
}
