use crate::deck::{Deck, Request, next_screen, requests};
use crate::images::Pictures;
use crate::others::{Preview as Shown, Previewed};
use crate::preview::Preview;
use crate::tokens::{DURATION_HAND_OVER, DURATION_STEP, SIDES};
use gpui::{
    AppContext, Context, DevicePixels, Entity, FocusHandle, KeyDownEvent, ScrollDelta, ScrollEvent, Size, Subscription,
    Task, Window,
};
use nova_desktop_info::{Desktop, Event as DesktopEvent};
use nova_look::Wheel;
use nova_look::motion::Eased;
use nova_look::tokens::{DURATION_FADE, DURATION_FAST, DURATION_PANEL};
use nova_stage::{Screen, Stage};
use nova_ui_client::{Daemon, Live, Refusal};
use nova_wallpaper_api::{Command, WallpaperFolder, Wallpapers};
use std::time::Instant;

pub const NAME: &str = "wallpaper";

const CARDS_KEPT: usize = 2 * SIDES + 7;

pub struct Carousel {
    pub(crate) screen: Screen,
    pub(crate) deck: Deck,
    pub(crate) focus: FocusHandle,
    pub(crate) position: Eased,
    pub(crate) opening: Eased,
    pub(crate) deck_shown: Eased,
    pub(crate) picking_here: Eased,
    pub(crate) loading: bool,
    pub(crate) problem: String,
    pub(crate) handing_over: bool,
    pub(crate) cards: Entity<Pictures>,
    pub(crate) minis: Entity<Pictures>,
    pub(crate) preview: Preview,
    pub(crate) arrivals: Vec<(usize, Eased)>,
    pub(crate) last_seen: Vec<(usize, f32)>,
    wheel: Wheel,
    was_open: bool,
    openings: u64,
    focus_key: String,
    desktop: Entity<Live<Desktop>>,
    previewed: Entity<Previewed>,
    asking: Option<Task<()>>,
    _followed: Vec<Subscription>,
}

impl Carousel {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let now = Instant::now();
        let desktop = Live::follow(nova_desktop_info::TOPIC, cx, |DesktopEvent::Compositor(desktop)| Some(desktop));
        let cards = cx.new(|_| Pictures::new(CARDS_KEPT));
        let minis = cx.new(|_| Pictures::new(CARDS_KEPT));
        let previewed = Previewed::get(cx);
        let followed = vec![
            cx.observe(&desktop, |carousel, _, cx| carousel.desktop_changed(cx)),
            cx.observe(&cards, |_, _, cx| cx.notify()),
            cx.observe(&minis, |_, _, cx| cx.notify()),
        ];
        Self {
            screen: screen.clone(),
            deck: Deck::new(&screen.name, Vec::new()),
            focus: cx.focus_handle(),
            position: Eased::lasting(0.0, DURATION_STEP, now),
            opening: Eased::at(0.0, now),
            deck_shown: Eased::lasting(1.0, DURATION_FAST, now),
            picking_here: Eased::lasting(0.0, DURATION_FAST, now),
            loading: false,
            problem: String::new(),
            handing_over: false,
            cards,
            minis,
            preview: Preview::new(cx),
            arrivals: Vec::new(),
            last_seen: Vec::new(),
            wheel: Wheel::default(),
            was_open: false,
            openings: 0,
            focus_key: String::new(),
            desktop,
            previewed,
            asking: None,
            _followed: followed,
        }
    }

    pub(crate) fn pixels(&self, window: &Window) -> Size<DevicePixels> {
        crate::draw::device_size(self.screen.size.width, self.screen.size.height, window)
    }

    pub(crate) fn moving(&self, now: Instant) -> bool {
        !(self.position.settled(now)
            && self.opening.settled(now)
            && self.deck_shown.settled(now)
            && self.picking_here.settled(now)
            && self.arrivals.iter().all(|(_, fade)| fade.settled(now)))
            || self.preview.moving(now)
    }

    pub(crate) fn follow_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open == self.was_open {
            return;
        }
        self.was_open = open;
        eprintln!("INFO nova-ui: wallpaper carousel {}", if open { "opened" } else { "closed" });
        let now = Instant::now();
        self.opening.set_target(if open { 1.0 } else { 0.0 }, now);
        if open {
            cx.defer_in(window, |carousel, _, cx| carousel.opened(cx));
            return;
        }
        self.publish(cx);
        let opening = self.openings;
        cx.spawn(async move |carousel, cx| {
            cx.background_executor().timer(DURATION_PANEL + DURATION_FADE).await;
            let _ = carousel.update(cx, |carousel, cx| {
                if !carousel.was_open && carousel.openings == opening {
                    carousel.faded(cx);
                }
            });
        })
        .detach();
    }

    fn opened(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.handing_over = false;
        self.deck_shown = Eased::lasting(1.0, DURATION_FAST, now);
        self.problem.clear();
        self.loading = true;
        self.deck.set_screens(screen_order(cx));
        self.deck.clear();
        self.focus_key = focus_key(self.desktop.read(cx).get());
        self.openings += 1;
        let opening = self.openings;
        let folder = Daemon::call::<WallpaperFolder>(Command::WallpaperFolder, cx);
        let shown = Daemon::call::<Wallpapers>(Command::Wallpapers, cx);
        self.asking = Some(cx.spawn(async move |carousel, cx| {
            let folder = folder.await;
            let shown = shown.await.unwrap_or_else(|refusal| {
                eprintln!("WARN nova-ui: wallpapers: {refusal}");
                Wallpapers::default()
            });
            let _ = carousel.update(cx, |carousel, cx| carousel.loaded(opening, folder, &shown, cx));
        }));
        self.publish(cx);
        cx.notify();
    }

    fn loaded(
        &mut self,
        opening: u64,
        folder: Result<WallpaperFolder, Refusal>,
        shown: &Wallpapers,
        cx: &mut Context<Self>,
    ) {
        if !self.was_open || opening != self.openings {
            return;
        }
        self.loading = false;
        match folder {
            Ok(listing) => {
                self.deck.load(listing, shown);
                self.position = Eased::lasting(self.deck.target as f32, DURATION_STEP, Instant::now());
                self.last_seen.clear();
                self.arrivals.clear();
            }
            Err(refusal) => self.problem = said(refusal),
        }
        self.publish(cx);
        cx.notify();
    }

    fn faded(&mut self, cx: &mut Context<Self>) {
        self.deck.clear();
        self.cards.update(cx, |pictures, _| pictures.clear());
        self.minis.update(cx, |pictures, _| pictures.clear());
        self.preview.show("", Default::default(), cx);
        self.release(None, cx);
        self.handing_over = false;
        self.asking = None;
        let here = self.screen.name.clone();
        self.previewed.update(cx, |previewed, cx| previewed.clear(&here, cx));
        cx.notify();
    }

    pub(crate) fn step(&mut self, by: i64, cx: &mut Context<Self>) {
        if self.handing_over || !self.deck.step(by) {
            return;
        }
        self.problem.clear();
        self.position.set_target(self.deck.target as f32, Instant::now());
        self.publish(cx);
        cx.notify();
    }

    pub(crate) fn choose_for(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        if self.handing_over || !self.deck.choose_for(name) {
            return;
        }
        self.problem.clear();
        self.position.set_target(self.deck.target as f32, Instant::now());
        self.publish(cx);
        cx.notify();
    }

    pub(crate) fn choose(&mut self, cx: &mut Context<Self>) {
        if self.deck.count() == 0 || self.handing_over {
            return;
        }
        let asked = requests(&self.deck.kept(), &self.deck.showing);
        if asked.is_empty() {
            self.close(cx);
            return;
        }
        self.handing_over = true;
        self.deck_shown.set_target(0.0, Instant::now());
        let opening = self.openings;
        self.asking = Some(cx.spawn(async move |carousel, cx| {
            let mut done = Vec::new();
            let mut failed = None;
            for request in asked {
                let command = Command::SetWallpaper { path: request.path.clone(), output: request.output.clone() };
                let answer = cx.update(|cx| Daemon::call::<()>(command, cx));
                if let Err(refusal) = answer.await {
                    failed = Some(said(refusal));
                    break;
                }
                done.push(request);
            }
            let _ = carousel.update(cx, |carousel, cx| carousel.applied(opening, &done, failed, cx));
        }));
        cx.notify();
    }

    fn applied(&mut self, opening: u64, done: &[Request], failed: Option<String>, cx: &mut Context<Self>) {
        if !self.handing_over || opening != self.openings {
            return;
        }
        if let Some(problem) = failed {
            self.deck.shown_now(done);
            self.handing_over = false;
            self.deck_shown.set_target(1.0, Instant::now());
            self.problem = problem;
            cx.notify();
            return;
        }
        let screen = self.screen.id;
        self.asking = Some(cx.spawn(async move |carousel, cx| {
            cx.background_executor().timer(DURATION_HAND_OVER).await;
            let still = carousel.update(cx, |carousel, _| carousel.was_open && carousel.openings == opening);
            if still.unwrap_or(false) {
                cx.update(|cx| Stage::close(NAME, screen, cx));
            }
        }));
    }

    pub(crate) fn release(&self, mut window: Option<&mut Window>, cx: &mut gpui::App) {
        self.preview.release(window.as_deref_mut(), cx);
        self.cards.update(cx, |pictures, cx| pictures.release(window.as_deref_mut(), cx));
        self.minis.update(cx, |pictures, cx| pictures.release(window, cx));
    }

    pub(crate) fn close(&self, cx: &mut Context<Self>) {
        Stage::close(NAME, self.screen.id, cx);
    }

    fn publish(&self, cx: &mut Context<Self>) {
        let here = self.screen.name.clone();
        let preview =
            Shown { paths: self.deck.previews(), choosing_for: self.deck.choosing_for.clone(), open: self.was_open };
        self.previewed.update(cx, |previewed, cx| previewed.set(&here, preview, cx));
    }

    fn desktop_changed(&mut self, cx: &mut Context<Self>) {
        let key = focus_key(self.desktop.read(cx).get());
        if key == self.focus_key {
            return;
        }
        self.focus_key = key;
        if self.was_open && !self.focus_key.is_empty() {
            self.close(cx);
        }
    }

    pub(crate) fn key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "left" | "up" => self.step(-1, cx),
            "right" | "down" => self.step(1, cx),
            "enter" => self.choose(cx),
            "tab" => {
                let by = if event.keystroke.modifiers.shift { -1 } else { 1 };
                let next = next_screen(self.deck.choosing_for.as_deref(), &self.deck.screens, by);
                self.choose_for(next, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    pub(crate) fn scrolled(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let sideways = match event.delta {
            ScrollDelta::Lines(lines) => lines.y == 0.0,
            ScrollDelta::Pixels(pixels) => pixels.y == gpui::px(0.0),
        };
        let notches = i64::from(self.wheel.notches(event));
        self.step(if sideways { notches } else { -notches }, cx);
    }
}

fn said(refusal: Refusal) -> String {
    match refusal {
        Refusal::Daemon(error) => error,
        other => other.to_string(),
    }
}

fn focus_key(desktop: Option<&Desktop>) -> String {
    let focused = desktop.and_then(|desktop| {
        desktop.outputs.iter().flat_map(|output| &output.workspaces).find(|workspace| workspace.focused)
    });
    focused.map_or_else(String::new, |workspace| {
        format!("{}/{}", workspace.id, workspace.active_window.as_deref().unwrap_or_default())
    })
}

fn screen_order(cx: &gpui::App) -> Vec<String> {
    let Some(screens) = Stage::screens(cx) else { return Vec::new() };
    let mut placed: Vec<(f32, f32, String)> = screens
        .read(cx)
        .list()
        .iter()
        .map(|screen| {
            let origin = cx.find_display(screen.id).map(|display| display.bounds().origin).unwrap_or_default();
            (f32::from(origin.x), f32::from(origin.y), screen.name.to_string())
        })
        .collect();
    placed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    placed.into_iter().map(|(_, _, name)| name).collect()
}
