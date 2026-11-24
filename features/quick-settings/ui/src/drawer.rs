use crate::NAME;
use crate::logic;
use gpui::{Context, Entity, SharedString, Subscription, Task};
use nova_host_info::{Event, Host, TOPIC};
use nova_look::motion::Eased;
use nova_look::tokens::{DURATION_FADE, DURATION_PANEL};
use nova_stage::{Parts, Screen, Shown, Stage};
use nova_ui_client::Live;
use std::time::{Duration, Instant};

pub const TILES: &str = "quick-settings.tiles";
pub const SLIDERS: &str = "quick-settings.sliders";
pub const CARDS: &str = "quick-settings.cards";
pub const FILL: &str = "quick-settings.fill";
pub const FOOT: &str = "quick-settings.foot";
pub const PAGES: &str = "quick-settings.pages";
pub use nova_look::PILL_BUTTONS;

const MINUTE: Duration = Duration::from_secs(60);

pub struct Drawer {
    pub screen: Screen,
    host: Entity<Live<Host>>,
    pub tiles: Parts,
    pub sliders: Parts,
    pub cards: Parts,
    pub fill: Parts,
    pub foot: Parts,
    pub pages: Parts,
    pub buttons: Parts,
    pub open: bool,
    pub opened_at: Instant,
    pub slides: Vec<(Option<SharedString>, Eased)>,
    back_home: Option<Task<()>>,
    minutes: Option<Task<()>>,
    _followed: Vec<Subscription>,
}

impl Drawer {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let host = Live::follow(TOPIC, cx, |Event::Host(host)| Some(host));
        let mut followed = vec![cx.observe(&host, |_, _, cx| cx.notify())];
        followed.extend(Stage::observe_panels(cx, |drawer, cx| drawer.follow_stage(cx)));
        followed.extend(Stage::slots(cx).map(|slots| cx.observe(&slots, |drawer, _, cx| drawer.follow_slots(cx))));
        followed.push(Shown::observe(cx, |drawer, cx| {
            if drawer.hosts().into_iter().fold(false, |changed, parts| parts.shown_changed() | changed) {
                cx.notify();
            }
        }));
        let now = Instant::now();
        Self {
            screen: screen.clone(),
            host,
            tiles: Parts::new(TILES, screen, cx),
            sliders: Parts::new(SLIDERS, screen, cx),
            cards: Parts::new(CARDS, screen, cx),
            fill: Parts::new(FILL, screen, cx),
            foot: Parts::new(FOOT, screen, cx),
            pages: Parts::new(PAGES, screen, cx),
            buttons: Parts::new(PILL_BUTTONS, screen, cx),
            open: false,
            opened_at: now,
            slides: vec![(None, Eased::lasting(1.0, logic::page_takes(), now))],
            back_home: None,
            minutes: None,
            _followed: followed,
        }
    }

    pub fn host<'a>(&self, cx: &'a gpui::App) -> Option<&'a Host> {
        self.host.read(cx).get()
    }

    fn follow_stage(&mut self, cx: &mut Context<Self>) {
        let screen = self.screen.id;
        let open = Stage::is_open(NAME, screen, cx);
        if open != self.open {
            self.open = open;
            if open {
                self.opened_at = Instant::now();
                self.back_home = None;
                self.tick_minutes(cx);
            } else {
                self.minutes = None;
                self.go_home_later(cx);
            }
            cx.notify();
        }
        let page = Stage::page(NAME, screen, cx);
        if page.as_ref().is_some_and(|page| self.pages.get(page).is_none()) {
            cx.defer(move |cx| Stage::show_page(NAME, screen, None, cx));
            return;
        }
        if self.shown_page() != page.as_ref() {
            self.turn_to(page, cx);
        }
    }

    pub fn shown_page(&self) -> Option<&SharedString> {
        self.slides.iter().find(|(_, slide)| slide.target() == 1.0).and_then(|(page, _)| page.as_ref())
    }

    fn turn_to(&mut self, page: Option<SharedString>, cx: &mut Context<Self>) {
        let now = Instant::now();
        if !self.slides.iter().any(|(each, _)| *each == page) {
            self.slides.push((page.clone(), Eased::lasting(0.0, logic::page_takes(), now)));
        }
        for (each, slide) in &mut self.slides {
            let target = if *each == page { 1.0 } else { 0.0 };
            match self.open {
                true => slide.set_target(target, now),
                false => *slide = Eased::lasting(target, logic::page_takes(), now),
            }
        }
        cx.notify();
    }

    fn go_home_later(&mut self, cx: &mut Context<Self>) {
        let screen = self.screen.id;
        self.back_home = Some(cx.spawn(async move |drawer, cx| {
            cx.background_executor().timer(DURATION_PANEL + DURATION_FADE).await;
            let _ = drawer.update(cx, |drawer, cx| {
                if !drawer.open {
                    Stage::show_page(NAME, screen, logic::FIRST_PAGE, cx);
                }
            });
        }));
    }

    fn tick_minutes(&mut self, cx: &mut Context<Self>) {
        self.minutes = Some(cx.spawn(async move |drawer, cx| {
            loop {
                let into = jiff::Timestamp::now().as_second().rem_euclid(MINUTE.as_secs() as i64);
                let left = MINUTE.saturating_sub(Duration::from_secs(into as u64));
                cx.background_executor().timer(left).await;
                if drawer.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        }));
    }

    fn hosts(&mut self) -> [&mut Parts; 7] {
        [
            &mut self.tiles,
            &mut self.sliders,
            &mut self.cards,
            &mut self.fill,
            &mut self.foot,
            &mut self.pages,
            &mut self.buttons,
        ]
    }

    fn follow_slots(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        for parts in self.hosts() {
            changed |= parts.sync(cx);
        }
        let gone = self.shown_page().is_some_and(|page| self.pages.get(page).is_none());
        if gone {
            let screen = self.screen.id;
            cx.defer(move |cx| Stage::show_page(NAME, screen, logic::FIRST_PAGE, cx));
        }
        if changed {
            cx.notify();
        }
    }
}
