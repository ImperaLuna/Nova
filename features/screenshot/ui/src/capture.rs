//! The `capture.overlay` view (host `shell.overlays`): the region picker on
//! every screen novad froze, while it's open (`nova screenshot region`),
//! and the card a saved shot lands in, on the screen that had focus when
//! it was saved. `ui/capture/CaptureOverlay.qml`, `ShotCards.qml` and
//! `service/ScreenshotService.qml` in today's shell.
//!
//! This is the link to novad and the state every screen shares: the
//! frozen frames, the toolbar's mode, a pick being saved, the card. Each
//! screen has a picker window ([`crate::picker`]) and a card window
//! ([`crate::card`]), opened when the overlay starts and kept, one pixel
//! big and taking nothing, while they show nothing: opening a window
//! stalls every window's frames, so none opens as the picker or the card
//! comes in.

use crate::card::CardWindow;
use crate::images;
use crate::pick::Countdown;
use crate::picker::PickerWindow;
use crate::tokens::{CARD_TIME, CARD_WIDTH, COUNTDOWN_TICK, THUMB_HEIGHT};
use gpui::{App, AppContext, Context, DisplayId, Entity, RenderImage, Size, Subscription, Task, WindowHandle, size};
use nova_desktop_info::{Desktop, Event as DesktopEvent};
use nova_look::motion::Eased;
use nova_look::tokens::{DURATION_PANEL, GAP, PAD};
use nova_screenshot_api::{
    CaptureFrame, CaptureMode, Command, Event, PixelRect, SavedScreenshot, ScreenshotAction, TOPIC,
};
use nova_stage::{Screen, Stage};
use nova_ui_client::{Daemon, Live};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// One screen's frozen frame, as novad captured it.
#[derive(Clone)]
pub struct Frame {
    pub output: String,
    pub image: Arc<RenderImage>,
    /// Its pixels, which `save_capture`'s rect counts in.
    pub pixels: Size<u32>,
}

/// The card on one screen: the saved shot, its picture once read, its
/// time, and how far in it has slid (1 in, 0 out).
pub struct Card {
    pub shot: SavedScreenshot,
    pub screen: DisplayId,
    pub thumb: Option<Arc<RenderImage>>,
    pub countdown: Countdown,
    pub slide: Eased,
}

pub struct Capture {
    /// The frames, while the picker is open; `None` while they are read.
    frames: Option<Vec<Frame>>,
    /// Counts novad's capture events, so frames read for an older one are
    /// dropped.
    generation: u64,
    mode: CaptureMode,
    /// A pick was sent and novad is saving it: the picker goes meanwhile.
    saving: bool,
    card: Option<Card>,
    /// A tick that drains the card's time is running.
    ticking: bool,
    pickers: Vec<(DisplayId, WindowHandle<PickerWindow>)>,
    cards: Vec<(DisplayId, WindowHandle<CardWindow>)>,
    /// Screens whose windows are about to open, and those whose windows
    /// couldn't (tried again when the screens change).
    opening: Vec<DisplayId>,
    failed: Vec<DisplayId>,
    desktop: Entity<Live<Desktop>>,
    _saving: Option<Task<()>>,
    _followed: Vec<Subscription>,
}

impl Capture {
    pub fn start(cx: &mut App) -> Entity<Self> {
        let capture = cx.new(|cx: &mut Context<Self>| {
            let this = cx.weak_entity();
            let events = Daemon::on_event(TOPIC, cx, move |event: Event, cx| {
                let _ = this.update(cx, |capture, cx| capture.take(event, cx));
            });
            let desktop = Live::follow(nova_desktop_info::TOPIC, cx, |DesktopEvent::Compositor(desktop)| Some(desktop));
            let mut followed = vec![events, cx.on_release(|capture: &mut Self, cx| capture.close(cx))];
            if let Some(screens) = Stage::screens(cx) {
                followed.push(cx.observe(&screens, |capture: &mut Self, _, cx| {
                    capture.failed.clear();
                    capture.sync_windows(cx);
                }));
            }
            Self {
                frames: None,
                generation: 0,
                mode: CaptureMode::Region,
                saving: false,
                card: None,
                ticking: false,
                pickers: Vec::new(),
                cards: Vec::new(),
                opening: Vec::new(),
                failed: Vec::new(),
                desktop,
                _saving: None,
                _followed: followed,
            }
        });
        capture.update(cx, |capture, cx| capture.sync_windows(cx));
        capture
    }

    fn take(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Capture { frames } => self.read_frames(frames, cx),
            Event::CaptureEnded => {
                self.generation += 1;
                self.drop_frames(cx);
            }
            Event::ScreenshotSaved(shot) => self.show_card(shot, cx),
        }
    }

    /// Reads novad's frames off the main thread; the picker opens once all
    /// are here, in the Region mode.
    fn read_frames(&mut self, frames: Vec<CaptureFrame>, cx: &mut Context<Self>) {
        self.generation += 1;
        self.drop_frames(cx);
        let generation = self.generation;
        let read = cx.background_spawn(async move {
            frames
                .into_iter()
                .filter_map(|frame| match images::frame(&PathBuf::from(&frame.path)) {
                    Ok(image) => {
                        let pixels = image.size(0);
                        let pixels = size(pixels.width.0.max(0) as u32, pixels.height.0.max(0) as u32);
                        Some(Frame { output: frame.output, image, pixels })
                    }
                    Err(error) => {
                        eprintln!("WARN nova-ui: the region picker has no frame for {}: {error}", frame.output);
                        None
                    }
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |capture, cx| {
            let frames = read.await;
            let _ = capture.update(cx, |capture, cx| {
                if capture.generation != generation {
                    drop_images(frames.into_iter().map(|frame| frame.image), cx);
                    return;
                }
                capture.frames = Some(frames);
                capture.mode = CaptureMode::Region;
                capture.saving = false;
                eprintln!("INFO nova-ui: region picker shown");
                cx.notify();
            });
        })
        .detach();
    }

    fn drop_frames(&mut self, cx: &mut Context<Self>) {
        self.saving = false;
        if let Some(frames) = self.frames.take() {
            eprintln!("INFO nova-ui: region picker hidden");
            drop_images(frames.into_iter().map(|frame| frame.image), cx);
        }
        cx.notify();
    }

    /// The frame the picker on the screen named `output` draws, while it
    /// shows.
    pub fn frame_for(&self, output: &str) -> Option<&Frame> {
        if self.saving {
            return None;
        }
        self.frames.as_ref()?.iter().find(|frame| frame.output == output)
    }

    pub fn mode(&self) -> CaptureMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: CaptureMode, cx: &mut Context<Self>) {
        if mode != self.mode {
            self.mode = mode;
            cx.notify();
        }
    }

    /// Saves `rect` of `output`'s frame. The picker goes at once, so the
    /// screens are live again while novad saves it, and comes back if
    /// novad couldn't.
    pub fn save(&mut self, output: &str, rect: PixelRect, cx: &mut Context<Self>) {
        if self.saving || rect.width == 0 || rect.height == 0 {
            return;
        }
        self.saving = true;
        cx.notify();
        let answer = Daemon::call::<()>(Command::SaveCapture { output: output.into(), rect }, cx);
        self._saving = Some(cx.spawn(async move |capture, cx| {
            if let Err(error) = answer.await {
                eprintln!("WARN nova-ui: saving the screenshot failed: {error}");
                let _ = capture.update(cx, |capture, cx| {
                    capture.saving = false;
                    cx.notify();
                });
            }
        }));
    }

    pub fn cancel(&self, cx: &App) {
        Daemon::send(Command::CancelCapture, cx);
    }

    /// The card comes in on the screen with focus; a new shot while it's
    /// out replaces it in place, with its full time.
    fn show_card(&mut self, shot: SavedScreenshot, cx: &mut Context<Self>) {
        let now = Instant::now();
        let kept = self.card.take().filter(|card| card.slide.target() > 0.0);
        let screen = match &kept {
            Some(card) => Some(card.screen),
            None => self.focused_screen(cx),
        };
        let slide = kept.as_ref().map_or_else(|| Eased::lasting(0.0, DURATION_PANEL, now), |card| card.slide);
        if let Some(thumb) = kept.and_then(|card| card.thumb) {
            drop_images([thumb], cx);
        }
        let Some(screen) = screen else { return };
        let mut card =
            Card { shot: shot.clone(), screen, thumb: None, countdown: Countdown::start(CARD_TIME, now), slide };
        card.slide.set_target(1.0, now);
        self.card = Some(card);
        self.read_thumb(shot, cx);
        self.keep_ticking(cx);
        cx.notify();
    }

    /// Reads the shot small, at twice the card's size for a sharp picture
    /// on a scaled screen.
    fn read_thumb(&mut self, shot: SavedScreenshot, cx: &mut Context<Self>) {
        let inner = CARD_WIDTH - (PAD + GAP / 2.0) * 2.0;
        let (width, height) = ((f32::from(inner) * 2.0) as u32, (f32::from(THUMB_HEIGHT) * 2.0) as u32);
        let path = PathBuf::from(&shot.path);
        let read = cx.background_spawn(async move { images::thumbnail(&path, width, height) });
        cx.spawn(async move |capture, cx| {
            let thumb = read.await;
            let _ = capture.update(cx, |capture, cx| match (thumb, capture.card.as_mut()) {
                (Ok(thumb), Some(card)) if card.shot.id == shot.id => {
                    card.thumb = Some(thumb);
                    cx.notify();
                }
                (Ok(thumb), _) => drop_images([thumb], cx),
                (Err(error), _) => eprintln!("WARN nova-ui: the screenshot card has no picture: {error}"),
            });
        })
        .detach();
    }

    pub fn card(&self) -> Option<&Card> {
        self.card.as_ref()
    }

    /// Moves the card on: it slides out once its time is up, and goes
    /// once it is out.
    pub fn step(&mut self, now: Instant, cx: &mut Context<Self>) {
        let Some(card) = &mut self.card else { return };
        if card.slide.target() > 0.0 && card.countdown.remaining(now) <= 0.0 {
            card.slide.set_target(0.0, now);
        }
        if card.slide.target() == 0.0
            && card.slide.settled(now)
            && let Some(thumb) = self.card.take().and_then(|card| card.thumb)
        {
            drop_images([thumb], cx);
        }
    }

    /// Runs a card button on the shown shot, and slides the card out.
    pub fn act(&mut self, action: ScreenshotAction, cx: &mut Context<Self>) {
        let Some(card) = &mut self.card else { return };
        if card.slide.target() == 0.0 {
            return;
        }
        Daemon::send(Command::ScreenshotAction { id: card.shot.id, action }, cx);
        card.slide.set_target(0.0, Instant::now());
        cx.notify();
    }

    /// The pointer on the card holds its time.
    pub fn hold(&mut self, held: bool, cx: &mut Context<Self>) {
        let Some(card) = &mut self.card else { return };
        card.countdown.set_held(held, Instant::now());
        self.keep_ticking(cx);
        cx.notify();
    }

    /// Redraws the draining line on a slow tick of its own while it drains.
    fn keep_ticking(&mut self, cx: &mut Context<Self>) {
        if self.ticking || !self.draining(Instant::now()) {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |capture, cx| {
            let mut draining = true;
            while draining {
                cx.background_executor().timer(COUNTDOWN_TICK).await;
                draining = capture
                    .update(cx, |capture, cx| {
                        capture.redraw_card(cx);
                        capture.ticking = capture.draining(Instant::now());
                        capture.ticking
                    })
                    .unwrap_or(false);
            }
        })
        .detach();
    }

    /// Redraws the card's window alone: a notify here would redraw every
    /// window that reads the capture, the pickers too.
    fn redraw_card(&self, cx: &mut App) {
        let Some(screen) = self.card.as_ref().map(|card| card.screen) else { return };
        if let Some((_, window)) = self.cards.iter().find(|(id, _)| *id == screen) {
            let _ = window.update(cx, |_, _, cx| cx.notify());
        }
    }

    fn draining(&self, now: Instant) -> bool {
        self.card.as_ref().is_some_and(|card| card.slide.target() > 0.0 && card.countdown.running(now))
    }

    /// The screen with focus, or the first.
    fn focused_screen(&self, cx: &App) -> Option<DisplayId> {
        let desktop = self.desktop.read(cx).get();
        let output = desktop.and_then(|desktop| desktop.outputs.iter().find(|output| output.focused));
        let screens = Stage::screens(cx)?;
        screens.read(cx).for_output(output.map(|output| output.name.as_str())).map(|screen| screen.id)
    }

    /// Opens a picker and a card window on each screen that has none, and
    /// closes those whose screen left.
    fn sync_windows(&mut self, cx: &mut Context<Self>) {
        let Some(screens) = Stage::screens(cx) else { return };
        let here = screens.read(cx).list().to_vec();
        let kept = |screen: &DisplayId| here.iter().any(|kept| kept.id == *screen);
        keep_windows(&mut self.pickers, kept, cx);
        keep_windows(&mut self.cards, kept, cx);
        if self.card.as_ref().is_some_and(|card| !kept(&card.screen)) {
            // Its screen left: it goes with it.
            if let Some(thumb) = self.card.take().and_then(|card| card.thumb) {
                drop_images([thumb], cx);
            }
        }
        let new: Vec<Screen> = here
            .iter()
            .filter(|screen| {
                !self.pickers.iter().any(|(id, _)| *id == screen.id)
                    && !self.opening.contains(&screen.id)
                    && !self.failed.contains(&screen.id)
            })
            .cloned()
            .collect();
        if new.is_empty() {
            return;
        }
        // Opened after this update: a new window draws at once, and its
        // drawing reads the capture.
        self.opening.extend(new.iter().map(|screen| screen.id));
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            let opened: Vec<_> = new
                .iter()
                .map(|screen| {
                    let picker = PickerWindow::open(screen, this.clone(), cx);
                    let card = CardWindow::open(screen, this.clone(), cx);
                    (screen.id, picker, card)
                })
                .collect();
            let _ = this.update(cx, |capture, cx| capture.opened(opened, cx));
        });
    }

    #[expect(clippy::type_complexity, reason = "the windows a deferred open made, by screen")]
    fn opened(
        &mut self,
        opened: Vec<(DisplayId, Option<WindowHandle<PickerWindow>>, Option<WindowHandle<CardWindow>>)>,
        cx: &mut Context<Self>,
    ) {
        for (screen, picker, card) in opened {
            self.opening.retain(|id| *id != screen);
            match (picker, card) {
                (Some(picker), Some(card)) => {
                    self.pickers.push((screen, picker));
                    self.cards.push((screen, card));
                }
                (picker, card) => {
                    close_window(picker, cx);
                    close_window(card, cx);
                    self.failed.push(screen);
                }
            }
        }
        self.sync_windows(cx);
    }

    fn close(&mut self, cx: &mut App) {
        for (_, window) in self.pickers.drain(..) {
            close_window(Some(window), cx);
        }
        for (_, window) in self.cards.drain(..) {
            close_window(Some(window), cx);
        }
        let frames = self.frames.take().into_iter().flatten().map(|frame| frame.image);
        let thumb = self.card.take().and_then(|card| card.thumb);
        drop_images(frames.chain(thumb), cx);
    }
}

/// Keeps the windows whose screen is still here, and closes the rest.
fn keep_windows<V: 'static>(
    windows: &mut Vec<(DisplayId, WindowHandle<V>)>,
    kept: impl Fn(&DisplayId) -> bool,
    cx: &mut App,
) {
    windows.retain(|(screen, window)| {
        let keep = kept(screen) && window.update(cx, |_, _, _| ()).is_ok();
        if !keep {
            close_window(Some(*window), cx);
        }
        keep
    });
}

fn close_window<V: 'static>(window: Option<WindowHandle<V>>, cx: &mut App) {
    if let Some(window) = window {
        let _ = window.update(cx, |_, window, _| window.remove_window());
    }
}

/// Frees what the windows hold of images no longer drawn.
fn drop_images(images: impl IntoIterator<Item = Arc<RenderImage>>, cx: &mut App) {
    for image in images {
        cx.drop_image(image, None);
    }
}
