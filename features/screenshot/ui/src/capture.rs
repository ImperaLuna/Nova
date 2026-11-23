mod record;

use crate::card::CardWindow;
use crate::images;
use crate::marks::MarksWindow;
use crate::pick::Countdown;
use crate::picker::PickerWindow;
use crate::tokens::{CARD_TIME, CARD_WIDTH, COUNTDOWN_TICK, FAILED_CARD_TIME, THUMB_HEIGHT};
use gpui::{App, AppContext, Context, DisplayId, Entity, RenderImage, Size, Subscription, Task, WindowHandle, size};
use nova_daemon_info::{Event as FeaturesEvent, FeatureState, FeatureStatus, topics};
use nova_desktop_info::{Desktop, Event as DesktopEvent};
use nova_look::motion::Eased;
use nova_look::tokens::{DURATION_PANEL, GAP, PAD};
use nova_recording_api::{ActiveRecording, LayoutRect, SavedRecording};
use nova_screenshot_api::{
    CaptureFrame, CaptureMode, Command, Event, PixelRect, SavedScreenshot, ScreenshotAction, TOPIC,
};
use nova_stage::{Screen, Stage};
use nova_ui_client::{Daemon, Live};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct Frame {
    pub output: String,
    pub image: Arc<RenderImage>,
    pub pixels: Size<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Saved {
    Shot(SavedScreenshot),
    Recording(SavedRecording),
    Failed { reason: String, started: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    File(ScreenshotAction),
    Retry,
    Log,
}

pub struct Card {
    pub saved: Saved,
    pub screen: DisplayId,
    pub thumb: Option<Arc<RenderImage>>,
    pub countdown: Countdown,
    pub slide: Eased,
}

pub struct Picking<'a> {
    pub frame: Option<&'a Frame>,
    pub mode: CaptureMode,
    pub video: bool,
    pub busy: bool,
    pub countdown: u32,
    pub can_switch: bool,
    pub switched: bool,
    pub desktop_audio: bool,
    pub mic_audio: bool,
}

struct Windows {
    picker: WindowHandle<PickerWindow>,
    card: WindowHandle<CardWindow>,
    marks: WindowHandle<MarksWindow>,
}

impl Windows {
    fn open(screen: &Screen, capture: gpui::WeakEntity<Capture>, cx: &mut App) -> Option<Self> {
        let picker = PickerWindow::open(screen, capture.clone(), cx);
        let card = CardWindow::open(screen, capture.clone(), cx);
        let marks = MarksWindow::open(screen, capture, cx);
        match (picker, card, marks) {
            (Some(picker), Some(card), Some(marks)) => Some(Self { picker, card, marks }),
            (picker, card, marks) => {
                close_window(picker, cx);
                close_window(card, cx);
                close_window(marks, cx);
                None
            }
        }
    }

    fn alive(&self, cx: &mut App) -> bool {
        self.picker.update(cx, |_, _, _| ()).is_ok()
            && self.card.update(cx, |_, _, _| ()).is_ok()
            && self.marks.update(cx, |_, _, _| ()).is_ok()
    }

    fn set_screen(&self, screen: &Screen, cx: &mut App) {
        let _ = self.picker.update(cx, |picker, window, cx| picker.set_screen(screen, window, cx));
        let _ = self.card.update(cx, |card, _, cx| card.set_screen(screen, cx));
        let _ = self.marks.update(cx, |marks, _, cx| marks.set_screen(screen, cx));
    }

    fn close(self, cx: &mut App) {
        close_window(Some(self.picker), cx);
        close_window(Some(self.card), cx);
        close_window(Some(self.marks), cx);
    }
}

pub struct Capture {
    frames: Option<Vec<Frame>>,
    generation: u64,
    mode: CaptureMode,
    saving: bool,
    switched: bool,
    next_mode: Option<CaptureMode>,
    ready: Ready,
    record: record::Record,
    card: Option<Card>,
    ticking: bool,
    windows: Vec<(DisplayId, Windows)>,
    opening: Vec<DisplayId>,
    failed: Vec<DisplayId>,
    desktop: Entity<Live<Desktop>>,
    _features: Entity<Live<Ready>>,
    _saving: Option<Task<()>>,
    _followed: Vec<Subscription>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Ready {
    screenshot: bool,
    recording: bool,
}

impl Ready {
    fn of(features: &[FeatureState]) -> Self {
        let ready =
            |id: &str| features.iter().any(|feature| feature.id == id && feature.status == FeatureStatus::Ready);
        Self { screenshot: ready("screenshot"), recording: ready("recording") }
    }
}

impl Capture {
    pub fn start(cx: &mut App) -> Entity<Self> {
        let capture = cx.new(|cx: &mut Context<Self>| {
            let this = cx.weak_entity();
            let events = Daemon::on_event(TOPIC, cx, move |event: Event, cx| {
                let _ = this.update(cx, |capture, cx| capture.take(event, cx));
            });
            let desktop = Live::follow(nova_desktop_info::TOPIC, cx, |DesktopEvent::Compositor(desktop)| Some(desktop));
            let features = Live::follow(topics::FEATURES, cx, |event| match event {
                FeaturesEvent::Features { features } => Some(Ready::of(&features)),
                _ => None,
            });
            let mut followed = vec![
                events,
                record::follow(cx),
                cx.observe(&features, |capture: &mut Self, features, cx| {
                    let ready = features.read(cx).get().copied().unwrap_or_default();
                    capture.follow_ready(ready, cx);
                }),
                cx.on_release(|capture: &mut Self, cx| capture.close(cx)),
            ];
            if let Some(connection) = Daemon::connection(cx) {
                followed.push(cx.observe(&connection, |capture: &mut Self, connection, cx| {
                    capture.follow_connection(connection.read(cx).is_connected(), cx);
                }));
            }
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
                switched: false,
                next_mode: None,
                ready: Ready { screenshot: true, recording: true },
                record: record::Record::default(),
                card: None,
                ticking: false,
                windows: Vec::new(),
                opening: Vec::new(),
                failed: Vec::new(),
                desktop,
                _features: features,
                _saving: None,
                _followed: followed,
            }
        });
        capture.update(cx, |capture, cx| capture.sync_windows(cx));
        capture
    }

    fn follow_connection(&mut self, connected: bool, cx: &mut Context<Self>) {
        if !connected {
            self.generation += 1;
            if self.frames.is_some() {
                self.drop_frames(cx);
            }
            self.cancel_recording_pick(cx);
            self.record.held = false;
        }
    }

    fn follow_ready(&mut self, ready: Ready, cx: &mut Context<Self>) {
        if ready == self.ready {
            return;
        }
        self.ready = ready;
        if !ready.recording {
            self.cancel_recording_pick(cx);
        }
        cx.notify();
    }

    fn take(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Capture { frames } => self.read_frames(frames, cx),
            Event::CaptureEnded => {
                self.generation += 1;
                self.drop_frames(cx);
            }
            Event::ScreenshotSaved(shot) => self.show_card(Saved::Shot(shot), cx),
        }
    }

    fn read_frames(&mut self, frames: Vec<CaptureFrame>, cx: &mut Context<Self>) {
        self.generation += 1;
        self.forget_frames(cx);
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
            let _ = capture.update(cx, |capture, cx| capture.frames_read(frames, generation, cx));
        })
        .detach();
    }

    fn frames_read(&mut self, frames: Vec<Frame>, generation: u64, cx: &mut Context<Self>) {
        if self.generation != generation {
            drop_images(frames.into_iter().map(|frame| frame.image), cx);
            return;
        }
        if frames.is_empty() {
            Daemon::send(Command::CancelCapture, cx);
            self.frames_held(false, cx);
            return;
        }
        self.frames = Some(frames);
        self.saving = false;
        if self.record.held {
            self.frames_held(true, cx);
        } else {
            self.switched = self.next_mode.is_some();
            self.mode = self.next_mode.take().unwrap_or(CaptureMode::Region);
            eprintln!("INFO nova-ui: region picker shown");
        }
        cx.notify();
    }

    fn drop_frames(&mut self, cx: &mut Context<Self>) {
        let was_held = self.record.held;
        self.record.held = false;
        let shown = self.frames.is_some() && !was_held;
        self.forget_frames(cx);
        if shown {
            eprintln!("INFO nova-ui: region picker hidden");
        }
        if was_held {
            self.frames_held(false, cx);
        }
    }

    fn forget_frames(&mut self, cx: &mut Context<Self>) {
        self.saving = false;
        self.switched = false;
        if let Some(frames) = self.frames.take() {
            drop_images(frames.into_iter().map(|frame| frame.image), cx);
        }
        cx.notify();
    }

    pub fn picking(&self, output: &str) -> Option<Picking<'_>> {
        let can_switch = self.ready.screenshot && self.ready.recording && self.record.active.is_none();
        if self.record.picking {
            return Some(Picking {
                frame: None,
                mode: self.mode,
                video: true,
                busy: self.record.countdown > 0,
                countdown: self.record.countdown,
                can_switch,
                switched: self.record.switched,
                desktop_audio: self.record.desktop_audio,
                mic_audio: self.record.mic_audio,
            });
        }
        if self.record.held {
            return None;
        }
        Some(Picking {
            frame: Some(self.frame_for(output)?),
            mode: self.mode,
            video: false,
            busy: false,
            countdown: 0,
            can_switch,
            switched: self.switched,
            desktop_audio: self.record.desktop_audio,
            mic_audio: self.record.mic_audio,
        })
    }

    pub fn frame_for(&self, output: &str) -> Option<&Frame> {
        if self.saving {
            return None;
        }
        self.frames.as_ref()?.iter().find(|frame| frame.output == output)
    }

    pub fn picking_mode(&self) -> CaptureMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: CaptureMode, cx: &mut Context<Self>) {
        if mode != self.mode {
            self.mode = mode;
            cx.notify();
        }
    }

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

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.record.picking || self.record.opening {
            self.cancel_recording_pick(cx);
        } else {
            Daemon::send(Command::CancelCapture, cx);
        }
    }

    fn show_card(&mut self, saved: Saved, cx: &mut Context<Self>) {
        let now = Instant::now();
        let old = self.card.take();
        if let Some(thumb) = old.as_ref().and_then(|card| card.thumb.clone()) {
            drop_images([thumb], cx);
        }
        let kept = old.filter(|card| card.slide.target() > 0.0);
        let screen = match &kept {
            Some(card) => Some(card.screen),
            None => self.focused_screen(cx),
        };
        let Some(screen) = screen else { return };
        let slide = kept.as_ref().map_or_else(|| Eased::lasting(0.0, DURATION_PANEL, now), |card| card.slide);
        let time = if matches!(saved, Saved::Failed { .. }) { FAILED_CARD_TIME } else { CARD_TIME };
        let mut countdown = Countdown::start(time, now);
        countdown.set_held(kept.is_some_and(|card| card.countdown.held()), now);
        let mut card = Card { saved: saved.clone(), screen, thumb: None, countdown, slide };
        card.slide.set_target(1.0, now);
        self.card = Some(card);
        if let Saved::Shot(shot) = saved {
            self.read_thumb(shot, cx);
        }
        self.keep_ticking(cx);
        cx.notify();
    }

    fn read_thumb(&mut self, shot: SavedScreenshot, cx: &mut Context<Self>) {
        let inner = CARD_WIDTH - (PAD + GAP / 2.0) * 2.0;
        let (width, height) = ((f32::from(inner) * 2.0) as u32, (f32::from(THUMB_HEIGHT) * 2.0) as u32);
        let path = PathBuf::from(&shot.path);
        let read = cx.background_spawn(async move { images::thumbnail(&path, width, height) });
        cx.spawn(async move |capture, cx| {
            let thumb = read.await;
            let _ = capture.update(cx, |capture, cx| match (thumb, capture.card.as_mut()) {
                (Ok(thumb), Some(card)) if card.saved == Saved::Shot(shot) => {
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

    pub fn act(&mut self, button: Button, cx: &mut Context<Self>) {
        let Some(card) = &mut self.card else { return };
        if card.slide.target() == 0.0 {
            return;
        }
        card.slide.set_target(0.0, Instant::now());
        match (&card.saved, button) {
            (Saved::Shot(shot), Button::File(action)) => {
                Daemon::send(Command::ScreenshotAction { id: shot.id, action }, cx);
            }
            (Saved::Recording(saved), Button::File(action)) => {
                Daemon::send(nova_recording_api::Command::RecordingAction { id: saved.id, action }, cx);
            }
            (Saved::Failed { .. }, Button::Retry) => self.record_again(cx),
            (Saved::Failed { .. }, Button::Log) => Daemon::send(nova_recording_api::Command::OpenLog, cx),
            _ => {}
        }
        cx.notify();
    }

    pub fn hold(&mut self, held: bool, cx: &mut Context<Self>) {
        let Some(card) = &mut self.card else { return };
        card.countdown.set_held(held, Instant::now());
        self.keep_ticking(cx);
        cx.notify();
    }

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

    fn redraw_card(&self, cx: &mut App) {
        let Some(screen) = self.card.as_ref().map(|card| card.screen) else { return };
        if let Some((_, windows)) = self.windows.iter().find(|(id, _)| *id == screen) {
            let _ = windows.card.update(cx, |_, _, cx| cx.notify());
        }
    }

    fn draining(&self, now: Instant) -> bool {
        self.card.as_ref().is_some_and(|card| card.slide.target() > 0.0 && card.countdown.running(now))
    }

    fn focused_screen(&self, cx: &App) -> Option<DisplayId> {
        let desktop = self.desktop.read(cx).get();
        let output = desktop.and_then(|desktop| desktop.outputs.iter().find(|output| output.focused));
        let screens = Stage::screens(cx)?;
        screens.read(cx).for_output(output.map(|output| output.name.as_str())).map(|screen| screen.id)
    }

    fn sync_windows(&mut self, cx: &mut Context<Self>) {
        let Some(screens) = Stage::screens(cx) else { return };
        let here = screens.read(cx).list().to_vec();
        for (screen, windows) in std::mem::take(&mut self.windows) {
            if here.iter().any(|kept| kept.id == screen) && windows.alive(cx) {
                self.windows.push((screen, windows));
            } else {
                windows.close(cx);
            }
        }
        for screen in &here {
            if let Some((_, windows)) = self.windows.iter().find(|(id, _)| *id == screen.id) {
                windows.set_screen(screen, cx);
            }
        }
        if self.card.as_ref().is_some_and(|card| !here.iter().any(|screen| screen.id == card.screen))
            && let Some(thumb) = self.card.take().and_then(|card| card.thumb)
        {
            drop_images([thumb], cx);
        }
        let new: Vec<Screen> = here
            .iter()
            .filter(|screen| {
                !self.windows.iter().any(|(id, _)| *id == screen.id)
                    && !self.opening.contains(&screen.id)
                    && !self.failed.contains(&screen.id)
            })
            .cloned()
            .collect();
        if new.is_empty() {
            return;
        }
        self.opening.extend(new.iter().map(|screen| screen.id));
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            let opened: Vec<_> =
                new.iter().map(|screen| (screen.id, Windows::open(screen, this.clone(), cx))).collect();
            let _ = this.update(cx, |capture, cx| capture.opened(opened, cx));
        });
    }

    fn opened(&mut self, opened: Vec<(DisplayId, Option<Windows>)>, cx: &mut Context<Self>) {
        for (screen, windows) in opened {
            self.opening.retain(|id| *id != screen);
            match windows {
                Some(windows) => self.windows.push((screen, windows)),
                None => self.failed.push(screen),
            }
        }
        self.sync_windows(cx);
    }

    fn close(&mut self, cx: &mut App) {
        for (_, windows) in self.windows.drain(..) {
            windows.close(cx);
        }
        let frames = self.frames.take().into_iter().flatten().map(|frame| frame.image);
        let thumb = self.card.take().and_then(|card| card.thumb);
        drop_images(frames.chain(thumb), cx);
    }

    pub fn recording(&self) -> Option<&ActiveRecording> {
        self.record.active.as_ref()
    }
}

pub fn on_screen(region: LayoutRect, screen: &Screen) -> gpui::Bounds<gpui::Pixels> {
    let origin = gpui::point(gpui::px(region.x as f32) - screen.origin.x, gpui::px(region.y as f32) - screen.origin.y);
    gpui::bounds(origin, gpui::size(gpui::px(region.width as f32), gpui::px(region.height as f32)))
}

fn close_window<V: 'static>(window: Option<WindowHandle<V>>, cx: &mut App) {
    if let Some(window) = window {
        let _ = window.update(cx, |_, window, _| window.remove_window());
    }
}

fn drop_images(images: impl IntoIterator<Item = Arc<RenderImage>>, cx: &mut App) {
    for image in images {
        cx.drop_image(image, None);
    }
}
