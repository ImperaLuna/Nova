//! `bluetooth.pairing`: a pairing question while the Bluetooth page isn't
//! there to ask it (a phone pairing with this computer, or a device you
//! started pairing before closing quick settings). A card on the overlay
//! layer under the right pill of the screen that had focus when the
//! question came, like the notifications' pop-ups, taking no space, and
//! the keyboard only when clicked (a PIN to type). It goes once the
//! question is answered or over. `PairingPopup.qml` in today's shell.
//!
//! Each screen has a window of its own, opened once when the overlay
//! starts (opening one stalls every window's frames) and only as big as
//! the card plus its shadow; only the one on the asking screen draws, and
//! at rest it commits nothing.

use crate::logic;
use crate::prompt;
use crate::tokens::{POPUP_RADIUS, POPUP_WIDTH};
use gpui::{
    App, AppContext, Bounds, Context, DisplayId, Entity, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Pixels, Render, Styled, Subscription, WeakEntity, Window, WindowHandle, canvas, div,
    layer_shell::{Anchor, KeyboardInteractivity, Layer},
    px, size,
};
use nova_bluetooth_api::{Bluetooth, Event, PairingRequest, TOPIC};
use nova_desktop_info::{Desktop, Event as DesktopEvent};
use nova_look::tokens::{BAR_GAP, BAR_HEIGHT, BAR_MARGIN, GAP, ICON_SIZE_SMALL, PAD, SHADOW_ROOM};
use nova_look::{Glass, Glyph, Icon, LookStyled, Shape, ShellWindow, TextField, Theme, region, set_blur_region};
use nova_stage::{FrameLog, Screen, Stage};
use nova_ui_client::{Live, Watch};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

/// The pop-ups' layer surfaces, for the compositor's rules (niri blurs
/// `^nova-`).
const NAMESPACE: &str = "nova-bluetooth";
/// A window with nothing to show: as small as the compositor takes.
const RESTING_HEIGHT: Pixels = px(1.0);
/// The card's top: under the bar, a gap below it.
fn top() -> Pixels {
    BAR_GAP + BAR_HEIGHT + GAP
}

/// The question and the screen it's asked on, for every screen's window.
pub struct Pairing {
    bluetooth: Entity<Live<Bluetooth>>,
    watch: Entity<Watch>,
    desktop: Entity<Live<Desktop>>,
    /// The screen it shows on, picked when a question comes.
    target: Option<DisplayId>,
    asking: bool,
    /// The question the PIN field was last typed for.
    pin_for: Option<u64>,
    /// A PIN being typed.
    pin: Entity<TextField>,
    windows: Vec<(DisplayId, WindowHandle<PairingWindow>)>,
    opening: Vec<DisplayId>,
    failed: Vec<DisplayId>,
    _followed: Vec<Subscription>,
}

impl Pairing {
    pub fn start(cx: &mut App) -> Entity<Self> {
        let pairing = cx.new(|cx: &mut Context<Self>| {
            let bluetooth = Live::follow(TOPIC, cx, |Event::Bluetooth(bluetooth)| Some(bluetooth));
            let desktop = Live::follow(nova_desktop_info::TOPIC, cx, |DesktopEvent::Compositor(desktop)| Some(desktop));
            let watch = crate::watch(cx);
            let pin = cx.new(TextField::new);
            let mut followed = vec![
                cx.observe(&bluetooth, |pairing: &mut Self, _, cx| pairing.follow(cx)),
                cx.observe(&watch, |pairing: &mut Self, _, cx| pairing.follow(cx)),
                cx.observe(&pin, |_, _, cx| cx.notify()),
                cx.on_release(|pairing: &mut Self, cx| pairing.close_windows(cx)),
            ];
            if let Some(screens) = Stage::screens(cx) {
                followed.push(cx.observe(&screens, |pairing: &mut Self, _, cx| {
                    pairing.failed.clear();
                    pairing.sync_windows(cx);
                }));
            }
            Self {
                bluetooth,
                watch,
                desktop,
                target: None,
                asking: false,
                pin_for: None,
                pin,
                windows: Vec::new(),
                opening: Vec::new(),
                failed: Vec::new(),
                _followed: followed,
            }
        });
        pairing.update(cx, |pairing, cx| {
            pairing.sync_windows(cx);
            pairing.follow(cx);
        });
        pairing
    }

    /// The question to ask here: none while the page asks it.
    pub fn request<'a>(&self, cx: &'a App) -> Option<&'a PairingRequest> {
        if self.watch.read(cx).is_wanted() {
            return None;
        }
        self.bluetooth.read(cx).get()?.pairing.as_ref()
    }

    /// What the card calls the device: its name, else its address.
    pub fn name(&self, request: &PairingRequest, cx: &App) -> String {
        let devices = self.bluetooth.read(cx).get().map_or(&[][..], |state| state.devices.as_slice());
        logic::device_name(devices, &request.address).to_owned()
    }

    pub fn pin(&self) -> &Entity<TextField> {
        &self.pin
    }

    /// A question came: it shows on the screen with focus. One that's over
    /// takes what was typed for it.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let id = self.request(cx).map(|request| request.id);
        let asking = id.is_some();
        if asking != self.asking {
            self.asking = asking;
            if asking {
                self.target = self.focused_screen(cx);
            }
        }
        // What was typed goes with its question.
        if id != self.pin_for {
            self.pin_for = id;
            if !self.pin.read(cx).text().is_empty() {
                self.pin.update(cx, |field, cx| field.set_text("", cx));
            }
        }
        cx.notify();
    }

    /// Whether `screen`'s window has a question to show.
    pub fn shows_on(&self, screen: DisplayId) -> bool {
        self.asking && self.target == Some(screen)
    }

    /// The screen with focus, or the first.
    fn focused_screen(&self, cx: &App) -> Option<DisplayId> {
        let desktop = self.desktop.read(cx).get();
        let output = desktop.and_then(|desktop| desktop.outputs.iter().find(|output| output.focused));
        let screens = Stage::screens(cx)?;
        screens.read(cx).for_output(output.map(|output| output.name.as_str())).map(|screen| screen.id)
    }

    /// Opens a window on each screen that has none, and closes those whose
    /// screen left. A question whose screen left moves to the one with
    /// focus.
    fn sync_windows(&mut self, cx: &mut Context<Self>) {
        let Some(screens) = Stage::screens(cx) else { return };
        let here = screens.read(cx).list().to_vec();
        let mut windows = std::mem::take(&mut self.windows);
        windows.retain(|(screen, window)| {
            let kept = here.iter().any(|kept| kept.id == *screen) && window.update(cx, |_, _, _| ()).is_ok();
            if !kept {
                let _ = window.update(cx, |_, window, _| window.remove_window());
            }
            kept
        });
        self.windows = windows;
        let new: Vec<Screen> = here
            .iter()
            .filter(|screen| {
                !self.windows.iter().any(|(id, _)| *id == screen.id)
                    && !self.opening.contains(&screen.id)
                    && !self.failed.contains(&screen.id)
            })
            .cloned()
            .collect();
        if !new.is_empty() {
            // Opened after this update: a new window draws at once, and its
            // drawing may read this.
            self.opening.extend(new.iter().map(|screen| screen.id));
            let this = cx.weak_entity();
            cx.defer(move |cx| {
                let opened: Vec<_> =
                    new.iter().map(|screen| (screen.id, PairingWindow::open(screen, this.clone(), cx))).collect();
                let _ = this.update(cx, |pairing, cx| pairing.opened(opened, cx));
            });
        }
        if self.asking && self.target.is_none_or(|target| !here.iter().any(|screen| screen.id == target)) {
            self.target = self.focused_screen(cx);
            cx.notify();
        }
    }

    fn opened(&mut self, opened: Vec<(DisplayId, Option<WindowHandle<PairingWindow>>)>, cx: &mut Context<Self>) {
        for (screen, window) in opened {
            self.opening.retain(|id| *id != screen);
            match window {
                Some(window) => self.windows.push((screen, window)),
                None => self.failed.push(screen),
            }
        }
        self.sync_windows(cx);
    }

    fn close_windows(&mut self, cx: &mut App) {
        for (_, window) in self.windows.drain(..) {
            let _ = window.update(cx, |_, window, _| window.remove_window());
        }
    }
}

/// One screen's pop-up window.
pub struct PairingWindow {
    screen: Screen,
    pairing: WeakEntity<Pairing>,
    /// The question is this screen's to show, as the pairing last said.
    shows: bool,
    /// The card's height as last laid out.
    card_height: Rc<Cell<Pixels>>,
    /// The height it last asked for.
    height: Pixels,
    keyboard: KeyboardInteractivity,
    /// The card's shape this frame, and the one the compositor has.
    shapes: Rc<RefCell<Vec<Shape>>>,
    sent: Rc<RefCell<Option<Vec<Shape>>>>,
    frames: Option<FrameLog>,
    _followed: Option<Subscription>,
}

/// The window's width: the card, its shadow's room on the left, and the
/// bar's margin on the right.
fn window_width() -> Pixels {
    SHADOW_ROOM + POPUP_WIDTH + BAR_MARGIN
}

impl PairingWindow {
    fn open(screen: &Screen, pairing: WeakEntity<Pairing>, cx: &mut App) -> Option<WindowHandle<Self>> {
        let mut shell = ShellWindow::new(screen.id, size(window_width(), RESTING_HEIGHT));
        shell.layer = Layer::Overlay;
        shell.anchor = Anchor::TOP | Anchor::RIGHT;
        shell.namespace = NAMESPACE;
        let screen = screen.clone();
        let frames = Stage::logs_frames(cx)
            .then(|| FrameLog::new(format!("{}/bluetooth", screen.name), screen.refresh, Instant::now()));
        let opened = cx.open_window(shell.options(), move |_, cx| {
            cx.new(|cx: &mut Context<Self>| {
                // Whether there's a question to show is found out here, not
                // while drawing, so a window that shows none never reads
                // the pairing and draws nothing when it changes.
                let followed = pairing.upgrade().map(|pairing| cx.observe(&pairing, Self::follow));
                let shows = pairing.upgrade().is_some_and(|pairing| pairing.read(cx).shows_on(screen.id));
                Self {
                    screen,
                    pairing,
                    shows,
                    card_height: Rc::new(Cell::new(px(0.0))),
                    height: RESTING_HEIGHT,
                    keyboard: KeyboardInteractivity::None,
                    shapes: Rc::default(),
                    sent: Rc::default(),
                    frames,
                    _followed: followed,
                }
            })
        });
        match opened {
            Ok(window) => Some(window),
            Err(error) => {
                eprintln!("ERROR nova-ui: can't open the Bluetooth pairing window: {error:#}");
                None
            }
        }
    }

    /// The pairing changed: draws while the question is this screen's, and
    /// once more when it no longer is.
    fn follow(&mut self, pairing: Entity<Pairing>, cx: &mut Context<Self>) {
        let shows = pairing.read(cx).shows_on(self.screen.id);
        if shows || self.shows {
            self.shows = shows;
            cx.notify();
        }
    }

    /// Makes the window `height` tall: nothing on it moves.
    fn fit(&mut self, height: Pixels, window: &mut Window) {
        let height = if height < RESTING_HEIGHT { RESTING_HEIGHT } else { height.ceil() };
        if height != self.height {
            self.height = height;
            window.resize(size(window_width(), height));
        }
    }

    /// The keyboard only for a question that's typed, once clicked.
    fn keyboard(&mut self, wanted: KeyboardInteractivity, window: &mut Window) {
        if wanted != self.keyboard {
            self.keyboard = wanted;
            window.set_keyboard_interactivity(wanted);
        }
    }

    /// Hands this frame's shape to the compositor when it changed: it
    /// blurs behind it, and clicks land on it alone.
    fn regions(&self) -> impl IntoElement {
        let (shapes, sent) = (self.shapes.clone(), self.sent.clone());
        canvas(
            |_, _, _| {},
            move |_, (), window, _| {
                let drawn = shapes.take();
                if sent.borrow().as_ref() != Some(&drawn) {
                    set_blur_region(window, &drawn);
                    window.set_input_region(Some(&region(&drawn)));
                    sent.replace(Some(drawn));
                }
            },
        )
        .absolute()
        .size_0()
    }

    fn log_frame(&mut self) {
        if let Some(frames) = &mut self.frames {
            for line in frames.frame(false, Instant::now()) {
                eprintln!("INFO nova-ui: {line}");
            }
        }
    }

    /// Enter answers a typed question.
    fn key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "enter" {
            return;
        }
        let Some(pairing) = self.pairing.upgrade() else { return };
        let Some(request) = pairing.read(cx).request(cx).cloned() else { return };
        // Only a typed answer goes with Enter; a yes takes a click.
        if !logic::typed(&request.ask) {
            return;
        }
        let pin = pairing.read(cx).pin().clone();
        prompt::accept(&request, &pin, cx);
        cx.stop_propagation();
    }
}

impl Render for PairingWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.log_frame();
        let shown = self.pairing.upgrade().filter(|_| self.shows).and_then(|pairing| {
            let pairing = pairing.read(cx);
            let request = pairing.request(cx)?.clone();
            Some((pairing.name(&request, cx), request, pairing.pin().clone()))
        });
        let Some((name, request, pin)) = shown else {
            // Nothing to show: as small as it gets, taking no keyboard.
            self.keyboard(KeyboardInteractivity::None, window);
            self.fit(RESTING_HEIGHT, window);
            return div().child(self.regions());
        };
        let wanted =
            if logic::typed(&request.ask) { KeyboardInteractivity::OnDemand } else { KeyboardInteractivity::None };
        self.keyboard(wanted, window);
        let height = self.card_height.get();
        if height > px(0.0) {
            self.fit(top() + height + SHADOW_ROOM, window);
        }
        let bounds = Bounds::new(gpui::point(SHADOW_ROOM, top()), size(POPUP_WIDTH, height));
        self.shapes.borrow_mut().push(Shape::new(bounds, POPUP_RADIUS));
        let measured = self.card_height.clone();
        let measure = canvas(
            move |bounds, window, _| {
                if measured.replace(bounds.size.height) != bounds.size.height {
                    window.request_animation_frame();
                }
            },
            |_, (), _, _| {},
        )
        .absolute()
        .inset_0();
        let content = div()
            .relative()
            .p(PAD)
            .flex()
            .flex_col()
            .gap(GAP)
            .child(Icon::new(Glyph::BLUETOOTH).size(ICON_SIZE_SMALL).color(Theme::of(cx).primary))
            .child(prompt::prompt(&request, &name, &pin, cx));
        let card = div()
            .absolute()
            .top(top())
            .left(SHADOW_ROOM)
            .w(POPUP_WIDTH)
            .child(Glass::new(POPUP_RADIUS).build(cx).absolute().inset_0())
            .child(measure)
            .child(content);
        div().size_full().body_text(cx).on_key_down(cx.listener(Self::key_down)).child(card).child(self.regions())
    }
}
