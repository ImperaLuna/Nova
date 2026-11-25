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

const NAMESPACE: &str = "nova-bluetooth";
const RESTING_HEIGHT: Pixels = px(1.0);
fn top() -> Pixels {
    BAR_GAP + BAR_HEIGHT + GAP
}

pub struct Pairing {
    bluetooth: Entity<Live<Bluetooth>>,
    watch: Entity<Watch>,
    desktop: Entity<Live<Desktop>>,
    target: Option<DisplayId>,
    asking: bool,
    pin_for: Option<u64>,
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

    pub fn request<'a>(&self, cx: &'a App) -> Option<&'a PairingRequest> {
        if self.watch.read(cx).is_wanted() {
            return None;
        }
        self.bluetooth.read(cx).get()?.pairing.as_ref()
    }

    pub fn name(&self, request: &PairingRequest, cx: &App) -> String {
        let devices = self.bluetooth.read(cx).get().map_or(&[][..], |state| state.devices.as_slice());
        logic::device_name(devices, &request.address).to_owned()
    }

    pub fn pin(&self) -> &Entity<TextField> {
        &self.pin
    }

    fn follow(&mut self, cx: &mut Context<Self>) {
        let id = self.request(cx).map(|request| request.id);
        let asking = id.is_some();
        if asking != self.asking {
            self.asking = asking;
            if asking {
                self.target = self.focused_screen(cx);
            }
        }
        if id != self.pin_for {
            self.pin_for = id;
            let max = self.request(cx).map_or(0, |request| crate::logic::code_max(&request.ask));
            self.pin.update(cx, |field, cx| {
                field.set_max_chars(max);
                if !field.text().is_empty() {
                    field.set_text("", cx);
                }
            });
        }
        cx.notify();
    }

    pub fn shows_on(&self, screen: DisplayId) -> bool {
        self.asking && self.target == Some(screen)
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

pub struct PairingWindow {
    screen: Screen,
    pairing: WeakEntity<Pairing>,
    shows: bool,
    card_height: Rc<Cell<Pixels>>,
    height: Pixels,
    keyboard: KeyboardInteractivity,
    shapes: Rc<RefCell<Vec<Shape>>>,
    sent: Rc<RefCell<Option<Vec<Shape>>>>,
    frames: Option<FrameLog>,
    _followed: Option<Subscription>,
}

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

    fn follow(&mut self, pairing: Entity<Pairing>, cx: &mut Context<Self>) {
        let shows = pairing.read(cx).shows_on(self.screen.id);
        if shows || self.shows {
            self.shows = shows;
            cx.notify();
        }
    }

    fn fit(&mut self, height: Pixels, window: &mut Window) {
        let height = if height < RESTING_HEIGHT { RESTING_HEIGHT } else { height.ceil() };
        if height != self.height {
            self.height = height;
            window.resize(size(window_width(), height));
        }
    }

    fn keyboard(&mut self, wanted: KeyboardInteractivity, window: &mut Window) {
        if wanted != self.keyboard {
            self.keyboard = wanted;
            window.set_keyboard_interactivity(wanted);
        }
    }

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

    fn key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "enter" {
            return;
        }
        let Some(pairing) = self.pairing.upgrade() else { return };
        let Some(request) = pairing.read(cx).request(cx).cloned() else { return };
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
