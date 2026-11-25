use crate::capture::{Capture, Frame, Picking};
use crate::pick::{around, is_click, on_pixels, region, size_label, to_layout, to_pixels, under_region};
use crate::tokens::{
    BADGE_HEIGHT, BUTTON_HEIGHT, BUTTON_PAD, FONT_SIZE_COUNTDOWN, MIN_DRAG, OUTLINE, RECORD_DOT, veil,
};
use crate::toolbar::{Ask, Layout, Look, toolbar};
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, CursorStyle, FocusHandle, FontWeight, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point,
    Render, RenderImage, SharedString, StatefulInteractiveElement, Styled, Subscription, TextRun, WeakEntity, Window,
    WindowHandle, bounds, div, img,
    layer_shell::{Anchor, KeyboardInteractivity, Layer},
    point, px, size,
};
use nova_look::motion::Eased;
use nova_look::tokens::{
    CHIP_PAD, DURATION_FADE, DURATION_PANEL, FONT_MONO, FONT_SANS, FONT_SIZE, FONT_SIZE_SMALL, GAP, ICON_GAP,
    WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_look::{LookStyled, Shape, ShellWindow, Theme, set_blur_region};
use nova_screenshot_api::CaptureMode;
use nova_stage::{FrameLog, Screen, Stage};
use std::sync::Arc;
use std::time::Instant;

const NAMESPACE: &str = "nova-capture";
const RESTING: Pixels = px(1.0);

struct Shows {
    frame: Option<Frame>,
    mode: CaptureMode,
    video: bool,
    busy: bool,
    countdown: u32,
    can_switch: bool,
    switched: bool,
    desktop_audio: bool,
    mic_audio: bool,
}

impl From<Picking<'_>> for Shows {
    fn from(picking: Picking<'_>) -> Self {
        Self {
            frame: picking.frame.cloned(),
            mode: picking.mode,
            video: picking.video,
            busy: picking.busy,
            countdown: picking.countdown,
            can_switch: picking.can_switch,
            switched: picking.switched,
            desktop_audio: picking.desktop_audio,
            mic_audio: picking.mic_audio,
        }
    }
}

pub struct PickerWindow {
    screen: Screen,
    capture: WeakEntity<Capture>,
    focus: FocusHandle,
    shown: bool,
    grown: Eased,
    faded: Eased,
    knob: Eased,
    sounds: Eased,
    region: Option<Bounds<Pixels>>,
    from: Option<Point<Pixels>>,
    seen_mode: CaptureMode,
    recorded: Option<Bounds<Pixels>>,
    video: bool,
    hovered: bool,
    sent: Option<Bounds<Pixels>>,
    frames: Option<FrameLog>,
    _followed: Option<Subscription>,
}

impl PickerWindow {
    pub fn open(screen: &Screen, capture: WeakEntity<Capture>, cx: &mut App) -> Option<WindowHandle<Self>> {
        let mut shell = ShellWindow::new(screen.id, size(RESTING, RESTING));
        shell.layer = Layer::Overlay;
        shell.anchor = Anchor::TOP | Anchor::LEFT;
        shell.namespace = NAMESPACE;
        let screen = screen.clone();
        let frames = Stage::logs_frames(cx)
            .then(|| FrameLog::new(format!("{}/screenshot-picker", screen.name), screen.refresh, Instant::now()));
        let opened = cx.open_window(shell.options(), move |window, cx| {
            window.set_input_region(Some(&[]));
            cx.new(|cx: &mut Context<Self>| {
                let followed = capture.upgrade().map(|capture| cx.observe(&capture, |_, _, cx| cx.notify()));
                let now = Instant::now();
                Self {
                    screen,
                    capture,
                    focus: cx.focus_handle(),
                    shown: false,
                    grown: Eased::lasting(0.0, DURATION_PANEL, now),
                    faded: Eased::lasting(0.0, DURATION_FADE, now),
                    knob: Eased::lasting(0.0, DURATION_PANEL, now),
                    sounds: Eased::lasting(0.0, DURATION_PANEL, now),
                    region: None,
                    from: None,
                    seen_mode: CaptureMode::Region,
                    recorded: None,
                    video: false,
                    hovered: false,
                    sent: None,
                    frames,
                    _followed: followed,
                }
            })
        });
        match opened {
            Ok(window) => Some(window),
            Err(error) => {
                eprintln!("ERROR nova-ui: can't open the region picker's window: {error:#}");
                None
            }
        }
    }

    pub fn set_screen(&mut self, screen: &Screen, window: &mut Window, cx: &mut Context<Self>) {
        if self.screen != *screen {
            if self.shown && self.screen.size != screen.size {
                window.resize(screen.size);
            }
            self.screen = screen.clone();
            cx.notify();
        }
    }

    fn show(&mut self, shows: Option<&Shows>, now: Instant, window: &mut Window, cx: &mut Context<Self>) {
        let Some(shows) = shows else {
            if self.shown {
                self.shown = false;
                self.reset();
                window.resize(size(RESTING, RESTING));
                window.set_input_region(Some(&[]));
                window.set_keyboard_interactivity(KeyboardInteractivity::None);
                set_blur_region(window, &[]);
                self.sent = None;
            }
            return;
        };
        let own = if shows.video { 1.0 } else { 0.0 };
        if !self.shown {
            self.shown = true;
            self.reset();
            window.resize(self.screen.size);
            window.set_input_region(None);
            window.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            window.focus(&self.focus, cx);
            let start = if shows.switched { 1.0 } else { 0.0 };
            self.grown = Eased::lasting(start, DURATION_PANEL, now);
            self.grown.set_target(1.0, now);
            self.faded = Eased::lasting(start, DURATION_FADE, now);
            self.faded.set_target(1.0, now);
            let from = if shows.switched { 1.0 - own } else { own };
            self.knob = Eased::lasting(from, DURATION_PANEL, now);
            self.sounds = Eased::lasting(from, DURATION_PANEL, now);
            self.video = shows.video;
        }
        if shows.video != self.video {
            self.video = shows.video;
            self.reset();
        }
        self.knob.set_target(own, now);
        self.sounds.set_target(own, now);
    }

    fn reset(&mut self) {
        self.region = None;
        self.from = None;
        self.recorded = None;
        self.hovered = false;
    }

    fn with_capture(&self, cx: &mut Context<Self>, act: impl FnOnce(&mut Capture, &mut Context<Capture>)) {
        if let Some(capture) = self.capture.upgrade() {
            capture.update(cx, act);
        }
    }

    fn picking(&self, cx: &App) -> Option<Shows> {
        let capture = self.capture.upgrade()?;
        capture.read(cx).picking(&self.screen.name).map(Shows::from)
    }

    fn takes_picks(&self, cx: &App) -> bool {
        self.recorded.is_none() && self.picking(cx).is_some_and(|shows| !shows.busy)
    }

    fn key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => {
                self.with_capture(cx, |capture, cx| capture.cancel(cx));
                cx.stop_propagation();
            }
            "enter" if self.video && self.from.is_none() && self.takes_picks(cx) => {
                if let Some(region) = self.region {
                    self.start_recording(region, cx);
                }
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn mode(&self, cx: &App) -> CaptureMode {
        self.capture.upgrade().map_or(CaptureMode::Region, |capture| capture.read(cx).picking_mode())
    }

    fn press(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.takes_picks(cx) {
            return;
        }
        self.from = Some(event.position);
        if self.mode(cx) == CaptureMode::Region {
            self.region = Some(region(event.position, event.position));
        }
        cx.notify();
    }

    fn cancel(&mut self, _event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.recorded.is_none() {
            self.with_capture(cx, |capture, cx| capture.cancel(cx));
        }
    }

    fn drag(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let moved = self.from.filter(|_| self.region.is_some()).map(|from| region(from, event.position));
        if moved.is_some() {
            self.region = moved;
        }
        if moved.is_some() || !self.hovered {
            self.hovered = true;
            cx.notify();
        }
    }

    fn release(&mut self, _event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.from.take().is_none() {
            return;
        }
        let whole = bounds(point(px(0.0), px(0.0)), self.screen.size);
        match self.mode(cx) {
            CaptureMode::Screen if self.video => self.start_recording(whole, cx),
            CaptureMode::Screen => self.take(whole, window, cx),
            CaptureMode::Region => {
                let picked = self.region.filter(|region| !is_click(*region, MIN_DRAG));
                match picked {
                    Some(picked) if !self.video => {
                        self.region = None;
                        self.take(picked, window, cx);
                    }
                    Some(_) => {}
                    None => self.region = None,
                }
            }
        }
        cx.notify();
    }

    fn take(&mut self, picked: Bounds<Pixels>, _window: &mut Window, cx: &mut Context<Self>) {
        let output = self.screen.name.to_string();
        let width = f32::from(self.screen.size.width);
        self.with_capture(cx, move |capture, cx| {
            let Some(frame) = capture.frame_for(&output) else { return };
            let scale = if width > 0.0 { frame.pixels.width as f32 / width } else { 1.0 };
            let rect = to_pixels(picked, scale, frame.pixels);
            capture.save(&output, rect, cx);
        });
    }

    fn start_recording(&mut self, picked: Bounds<Pixels>, cx: &mut Context<Self>) {
        self.recorded = Some(picked);
        let whole = self.mode(cx) == CaptureMode::Screen;
        let region = (!whole).then(|| to_layout(picked, self.screen.origin));
        let output = self.screen.name.to_string();
        self.with_capture(cx, move |capture, cx| capture.record(output, region, cx));
        cx.notify();
    }

    fn ask(this: WeakEntity<Self>) -> impl Fn(Ask, &mut Window, &mut App) + Clone + 'static {
        move |ask, _, cx| {
            let _ = this.update(cx, |picker, cx| {
                picker.from = None;
                picker.with_capture(cx, |capture, cx| match ask {
                    Ask::Mode(_) | Ask::DesktopAudio | Ask::MicAudio if capture.counting() => {}
                    Ask::Mode(mode) => capture.set_mode(mode, cx),
                    Ask::Video(video) => capture.switch_kind(video, cx),
                    Ask::DesktopAudio => capture.toggle_desktop_audio(cx),
                    Ask::MicAudio => capture.toggle_mic_audio(cx),
                    Ask::Close => capture.cancel(cx),
                });
            });
        }
    }

    fn record_click(this: WeakEntity<Self>) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
        move |_, _, cx| {
            cx.stop_propagation();
            let _ = this.update(cx, |picker, cx| {
                if let Some(region) = picker.region.filter(|_| picker.recorded.is_none()) {
                    picker.start_recording(region, cx);
                }
            });
        }
    }

    fn blur(&mut self, glass: Bounds<Pixels>, window: &Window) {
        if self.sent != Some(glass) {
            set_blur_region(window, &[Shape::new(glass, glass.size.height / 2.0)]);
            self.sent = Some(glass);
        }
    }

    fn log_frame(&mut self, moving: bool, now: Instant) {
        if let Some(frames) = &mut self.frames {
            for line in frames.frame(moving, now) {
                eprintln!("INFO nova-ui: {line}");
            }
        }
    }
}

impl Render for PickerWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let picking = self.picking(cx);
        self.show(picking.as_ref(), now, window, cx);
        let Some(shows) = picking else {
            self.log_frame(false, now);
            return div().into_any_element();
        };
        if shows.mode != self.seen_mode {
            self.seen_mode = shows.mode;
            self.region = None;
        }
        let screen = self.screen.size;
        let layout = Layout::of(shows.can_switch, shows.video, window);
        let (grown, shown) = (self.grown.value(now), self.faded.value(now));
        let (knob, sounds) = (self.knob.value(now), self.sounds.value(now));
        let scale = window.scale_factor();
        let glass = on_pixels(layout.bounds(screen.width, grown, sounds), scale);
        self.blur(glass, window);
        let moving = [&self.grown, &self.faded, &self.knob, &self.sounds].iter().any(|eased| !eased.settled(now));
        if moving {
            window.request_animation_frame();
        }
        self.log_frame(moving, now);

        let whole = bounds(point(px(0.0), px(0.0)), screen);
        let clear = match (self.recorded, shows.mode) {
            (Some(recorded), _) => Some(recorded),
            (None, CaptureMode::Screen) if self.hovered => Some(whole),
            (None, CaptureMode::Screen) => None,
            (None, CaptureMode::Region) => self.region,
        };
        let theme = Theme::of(cx);
        let pieces = around(clear.unwrap_or_default(), screen, scale);
        let mut root = div()
            .id("picker")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .body_text(cx)
            .cursor(CursorStyle::Crosshair)
            .on_key_down(cx.listener(Self::key))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::press))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::cancel))
            .on_mouse_move(cx.listener(Self::drag))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::release))
            .on_hover(cx.listener(|picker, hovered: &bool, _, cx| {
                picker.hovered = *hovered;
                cx.notify();
            }))
            .children(shows.frame.as_ref().map(|frame| frame_around(frame, glass, screen, scale)).unwrap_or_default())
            .children(pieces.iter().filter(|piece| !piece.is_empty()).map(|piece| rect(*piece).bg(veil())));
        if let Some(clear) = clear.filter(|clear| !clear.is_empty()) {
            root =
                root.child(rect(on_pixels(clear, scale).dilate(OUTLINE)).border(OUTLINE).border_color(theme.primary));
        }
        let dragging = self.from.is_some();
        if let Some(region) = self.region.filter(|_| dragging) {
            root = root.child(badge(shows.frame.as_ref(), region, screen, window, cx));
        }
        if let Some(region) = self.region.filter(|_| shows.video && !dragging && self.recorded.is_none()) {
            root = root.child(
                record_button(region, screen, window, cx)
                    .on_mouse_down(MouseButton::Left, Self::record_click(cx.weak_entity())),
            );
        }
        if let Some(recorded) = self.recorded.filter(|_| shows.countdown > 0) {
            root = root.child(count(shows.countdown, recorded, cx));
        }
        let look = Look {
            glass,
            grown,
            shown,
            mode: shows.mode,
            video: shows.video,
            knob,
            sounds,
            desktop_audio: shows.desktop_audio,
            mic_audio: shows.mic_audio,
            busy: shows.busy || self.recorded.is_some(),
        };
        root.child(toolbar(&layout, &look, Self::ask(cx.weak_entity()), cx)).into_any_element()
    }
}

fn rect(at: Bounds<Pixels>) -> gpui::Div {
    div().absolute().left(at.origin.x).top(at.origin.y).w(at.size.width).h(at.size.height)
}

fn frame_around(frame: &Frame, glass: Bounds<Pixels>, screen: gpui::Size<Pixels>, scale: f32) -> Vec<AnyElement> {
    around(glass, screen, scale)
        .into_iter()
        .filter(|piece| !piece.is_empty())
        .map(|piece| {
            rect(piece)
                .overflow_hidden()
                .child(
                    img(Arc::<RenderImage>::clone(&frame.image))
                        .absolute()
                        .left(-piece.origin.x)
                        .top(-piece.origin.y)
                        .w(screen.width)
                        .h(screen.height),
                )
                .into_any_element()
        })
        .collect()
}

fn text_width(text: &str, family: nova_look::Family, weight: FontWeight, size: Pixels, window: &Window) -> Pixels {
    let mut face = gpui::font(family);
    face.weight = weight;
    let run = TextRun {
        len: text.len(),
        font: face,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window.text_system().shape_line(SharedString::from(text.to_owned()), size, &[run], None).width
}

fn badge(
    frame: Option<&Frame>,
    region: Bounds<Pixels>,
    screen: gpui::Size<Pixels>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let label = match frame {
        Some(frame) => {
            let scale = if screen.width > px(0.0) { frame.pixels.width as f32 / f32::from(screen.width) } else { 1.0 };
            let pixels = to_pixels(region, scale, frame.pixels);
            size_label(pixels.width, pixels.height)
        }
        None => {
            let layout = to_layout(region, point(px(0.0), px(0.0)));
            size_label(layout.width, layout.height)
        }
    };
    let text = text_width(&label, FONT_MONO, FontWeight::NORMAL, FONT_SIZE_SMALL, window);
    let item = size(text + CHIP_PAD * 2.0, BADGE_HEIGHT);
    let at = under_region(region, item, screen, GAP);
    rect(bounds(at, item))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme.surface())
        .font_family(FONT_MONO)
        .text_size(FONT_SIZE_SMALL)
        .text_color(theme.fg)
        .child(label)
        .into_any_element()
}

fn record_button(
    region: Bounds<Pixels>,
    screen: gpui::Size<Pixels>,
    window: &Window,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::of(cx);
    let (record, enter) = ("Record", "Enter");
    let width = RECORD_DOT
        + ICON_GAP
        + text_width(record, FONT_SANS, WEIGHT_LABEL, FONT_SIZE, window)
        + ICON_GAP
        + text_width(enter, FONT_MONO, FontWeight::NORMAL, FONT_SIZE_SMALL, window)
        + BUTTON_PAD * 2.0;
    let item = size(width, BUTTON_HEIGHT);
    let mut at = under_region(region, item, screen, GAP);
    at.x = at.x.min(screen.width - width).max(px(0.0));
    let raised = theme.surface_raised();
    rect(bounds(at, item))
        .id("record")
        .flex()
        .items_center()
        .justify_center()
        .gap(ICON_GAP)
        .rounded_full()
        .bg(theme.surface())
        .hover(move |style| style.bg(raised))
        .cursor(CursorStyle::PointingHand)
        .child(div().flex_none().size(RECORD_DOT).rounded_full().bg(theme.error))
        .child(div().font_weight(WEIGHT_LABEL).text_color(theme.fg).child(record))
        .child(div().font_family(FONT_MONO).text_size(FONT_SIZE_SMALL).text_color(theme.fg_dim()).child(enter))
}

fn count(countdown: u32, recorded: Bounds<Pixels>, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let number = countdown.to_string();
    let text = |color, dx: f32, dy: f32| {
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .left(px(dx))
            .top(px(dy))
            .right(px(-dx))
            .bottom(px(-dy))
            .text_color(color)
            .child(number.clone())
    };
    let mut drawn = rect(recorded).font_family(FONT_SANS).text_size(FONT_SIZE_COUNTDOWN).font_weight(WEIGHT_TITLE);
    for (dx, dy) in
        [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)]
    {
        drawn = drawn.child(text(veil(), dx, dy));
    }
    drawn.child(text(theme.fg, 0.0, 0.0)).into_any_element()
}
