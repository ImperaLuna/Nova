//! The carousel on one screen: what it holds, and what a key, a click, the
//! wheel and novad's answers do to it. `draw.rs` renders it.
//!
//! Over the whole screen, which previews the picked image, a deck of the
//! wallpaper folder's images in the middle, the picked one facing you and
//! the others turned away either side. It picks for every screen at once,
//! or (Tab, or a click in the screen picker under the deck) for one
//! screen, each screen previewing its own pick. Left and Right (or the
//! wheel) move through the images, Enter (or a click on the middle card)
//! keeps every screen's pick, Esc (or a click beside the deck) puts them
//! all back. Opened by `nova toggle wallpaper`.
//!
//! The stage owns the fade over the screen, Esc and the keyboard; this
//! view draws everything else and closes itself on a click beside the
//! deck.

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

/// The panel's name: the feature's id, which `nova toggle` uses.
pub const NAME: &str = "wallpaper";

/// The card pictures it holds: those in view, and a few it just passed.
const CARDS_KEPT: usize = 2 * SIDES + 7;

pub struct Carousel {
    pub(crate) screen: Screen,
    pub(crate) deck: Deck,
    pub(crate) focus: FocusHandle,
    /// Where the deck is on its way to `deck.target`.
    pub(crate) position: Eased,
    /// 0 closed, 1 open: the deck rises and spreads out as it opens.
    pub(crate) opening: Eased,
    /// The deck's opacity: it fades out once an image is picked.
    pub(crate) deck_shown: Eased,
    /// The mark on this screen while the deck picks for it alone.
    pub(crate) picking_here: Eased,
    /// Waiting for novad to list the folder, and why it couldn't, or why
    /// the pick didn't work.
    pub(crate) loading: bool,
    pub(crate) problem: String,
    /// Picked and handed to awww: the preview stays while awww fades the
    /// image in under it.
    pub(crate) handing_over: bool,
    pub(crate) cards: Entity<Pictures>,
    pub(crate) minis: Entity<Pictures>,
    pub(crate) preview: Preview,
    /// Cards fading in where they came round to, by image.
    pub(crate) arrivals: Vec<(usize, Eased)>,
    /// How much of each card showed in the last frame, by image.
    pub(crate) last_seen: Vec<(usize, f32)>,
    wheel: Wheel,
    was_open: bool,
    /// How many times it opened: an answer for an earlier opening is
    /// dropped.
    openings: u64,
    /// Where the compositor's focus is (the focused workspace and its
    /// window); a change while open closes it.
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

    /// The screen's size in pixels, which the preview is decoded at.
    pub(crate) fn pixels(&self, window: &Window) -> Size<DevicePixels> {
        crate::draw::device_size(self.screen.size.width, self.screen.size.height, window)
    }

    /// Whether anything of it moves at `now`.
    pub(crate) fn moving(&self, now: Instant) -> bool {
        !(self.position.settled(now)
            && self.opening.settled(now)
            && self.deck_shown.settled(now)
            && self.picking_here.settled(now)
            && self.arrivals.iter().all(|(_, fade)| fade.settled(now)))
            || self.preview.moving(now)
    }

    /// Called from `render` with whether the stage has the panel open:
    /// follows it opening and closing.
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
        // Kept as it closes, so a picked deck doesn't come back as it
        // fades; gone once it has.
        self.publish(cx);
        cx.spawn(async move |carousel, cx| {
            cx.background_executor().timer(DURATION_PANEL + DURATION_FADE).await;
            let _ = carousel.update(cx, |carousel, cx| {
                if !carousel.was_open {
                    carousel.faded(cx);
                }
            });
        })
        .detach();
    }

    /// As it opens: an empty deck while novad lists the folder.
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

    /// novad's answer to the `opening`th opening.
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
                // Straight to the screen's own image, without moving there.
                self.position = Eased::lasting(self.deck.target as f32, DURATION_STEP, Instant::now());
                self.last_seen.clear();
                self.arrivals.clear();
            }
            Err(refusal) => self.problem = said(refusal),
        }
        self.publish(cx);
        cx.notify();
    }

    /// Closed and faded: its images go, and the other screens' previews.
    fn faded(&mut self, cx: &mut Context<Self>) {
        self.deck.clear();
        self.cards.update(cx, |pictures, _| pictures.clear());
        self.minis.update(cx, |pictures, _| pictures.clear());
        self.preview.show("", Default::default(), cx);
        // Not drawn again until it opens: out of the GPU's atlas now.
        self.release(None, cx);
        self.handing_over = false;
        self.asking = None;
        let here = self.screen.name.clone();
        self.previewed.update(cx, |previewed, cx| previewed.clear(&here, cx));
        cx.notify();
    }

    /// Moves the deck `by` cards, round past either end.
    pub(crate) fn step(&mut self, by: i64, cx: &mut Context<Self>) {
        if self.handing_over || !self.deck.step(by) {
            return;
        }
        self.problem.clear();
        self.position.set_target(self.deck.target as f32, Instant::now());
        self.publish(cx);
        cx.notify();
    }

    /// Picks for screen `name` from now on, `None` for every one.
    pub(crate) fn choose_for(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        if self.handing_over || !self.deck.choose_for(name) {
            return;
        }
        self.problem.clear();
        self.position.set_target(self.deck.target as f32, Instant::now());
        self.publish(cx);
        cx.notify();
    }

    /// Keeps every screen's pick; with none changed, it closes.
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
        let calls: Vec<_> = asked.into_iter().map(|Request { path, output }| (path, output)).collect();
        self.asking = Some(cx.spawn(async move |carousel, cx| {
            let mut failed = None;
            for (path, output) in calls {
                let answer = cx.update(|cx| Daemon::call::<()>(Command::SetWallpaper { path, output }, cx));
                if let Err(refusal) = answer.await {
                    failed = Some(said(refusal));
                    break;
                }
            }
            let _ = carousel.update(cx, |carousel, cx| carousel.applied(opening, failed, cx));
        }));
        cx.notify();
    }

    /// novad's answer to the picks: closes once awww has faded them in.
    fn applied(&mut self, opening: u64, failed: Option<String>, cx: &mut Context<Self>) {
        // Put back with Esc since.
        if !self.handing_over || opening != self.openings {
            return;
        }
        if let Some(problem) = failed {
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

    /// Takes the pictures let go out of the GPU's atlas: from `window`
    /// too while it is being drawn.
    pub(crate) fn release(&self, mut window: Option<&mut Window>, cx: &mut gpui::App) {
        self.preview.release(window.as_deref_mut(), cx);
        self.cards.update(cx, |pictures, cx| pictures.release(window.as_deref_mut(), cx));
        self.minis.update(cx, |pictures, cx| pictures.release(window, cx));
    }

    pub(crate) fn close(&self, cx: &mut Context<Self>) {
        Stage::close(NAME, self.screen.id, cx);
    }

    /// Hands the other screens what they preview.
    fn publish(&self, cx: &mut Context<Self>) {
        let here = self.screen.name.clone();
        let preview =
            Shown { paths: self.deck.previews(), choosing_for: self.deck.choosing_for.clone(), open: self.was_open };
        self.previewed.update(cx, |previewed, cx| previewed.set(&here, preview, cx));
    }

    /// Focus moving behind it (Alt+Tab, Super+Tab to another workspace, a
    /// window opening) closes it as Esc does, rather than leaving it over
    /// windows switching behind it.
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

    /// A key. Esc goes on to the stage, which closes the panel.
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

    /// The wheel: one card a notch, down or right for the next.
    pub(crate) fn scrolled(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let sideways = match event.delta {
            ScrollDelta::Lines(lines) => lines.x.abs() > lines.y.abs(),
            ScrollDelta::Pixels(pixels) => pixels.x.abs() > pixels.y.abs(),
        };
        let notches = i64::from(self.wheel.notches(event));
        self.step(if sideways { notches } else { -notches }, cx);
    }
}

/// What a refusal says, for the label.
fn said(refusal: Refusal) -> String {
    match refusal {
        Refusal::Daemon(error) => error,
        other => other.to_string(),
    }
}

/// Where the compositor's focus is: the focused workspace and the window
/// on it (`Protocol.focusKey`), "" while it isn't known.
fn focus_key(desktop: Option<&Desktop>) -> String {
    let focused = desktop.and_then(|desktop| {
        desktop.outputs.iter().flat_map(|output| &output.workspaces).find(|workspace| workspace.focused)
    });
    focused.map_or_else(String::new, |workspace| {
        format!("{}/{}", workspace.id, workspace.active_window.as_deref().unwrap_or_default())
    })
}

/// Every screen's name, left to right as they are laid out.
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
