//! One screen's region picker: its frozen frame under a dim veil, and the
//! toolbar at the top. In the Region mode a drag outlines a region and
//! letting go takes it; in the Screen mode a click takes the whole screen,
//! and the screen under the pointer is the one outlined. Esc, a right
//! click or close ends it. `ui/capture/CaptureWindow.qml` and
//! `RegionPicker.qml` in today's shell.
//!
//! Its own window, on the overlay layer, so it covers fullscreen windows
//! and the bar too (the frozen frame shows them as they were). While no
//! pick is open it is one pixel big and takes no clicks and no keyboard;
//! while one is, it covers the screen and takes the keyboard for Esc. The
//! frame is cut out under the toolbar, so its glass blurs the live screen
//! there, not the frame's copy of the pill it stands in for.

use crate::capture::{Capture, Frame};
use crate::pick::{around, is_click, region, size_label, to_pixels, under_region};
use crate::tokens::{BADGE_HEIGHT, MIN_DRAG, OUTLINE, VEIL_OPACITY};
use crate::toolbar::{Ask, Layout, Look, toolbar};
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, CursorStyle, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render,
    RenderImage, StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, WindowHandle, bounds, div, hsla,
    img,
    layer_shell::{Anchor, KeyboardInteractivity, Layer},
    point, px, size,
};
use nova_look::motion::Eased;
use nova_look::tokens::{CHIP_PAD, DURATION_FADE, DURATION_PANEL, FONT_MONO, FONT_SIZE_SMALL, GAP};
use nova_look::{LookStyled, Shape, ShellWindow, Theme, set_blur_region};
use nova_screenshot_api::CaptureMode;
use nova_stage::{FrameLog, Screen, Stage};
use std::sync::Arc;
use std::time::Instant;

/// The picker's layer surfaces, for the compositor's rules (niri blurs
/// `^nova-`).
const NAMESPACE: &str = "nova-capture";
/// A window with no pick open: as small as the compositor takes.
const RESTING: Pixels = px(1.0);

pub struct PickerWindow {
    screen: Screen,
    capture: WeakEntity<Capture>,
    focus: FocusHandle,
    /// It covers the screen now (what the window was last set to).
    shown: bool,
    /// The toolbar growing in, and fading in.
    grown: Eased,
    faded: Eased,
    /// The region being dragged, where the drag started, and the mode it
    /// was dragged in.
    region: Option<Bounds<Pixels>>,
    from: Option<Point<Pixels>>,
    seen_mode: CaptureMode,
    /// The pointer is on this screen (the Screen mode outlines it).
    hovered: bool,
    /// The toolbar's glass as the compositor last got it.
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
                    region: None,
                    from: None,
                    seen_mode: CaptureMode::Region,
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

    /// Covers the screen and takes the keyboard while a pick is open, and
    /// gives both back after.
    fn show(&mut self, shown: bool, now: Instant, window: &mut Window, cx: &mut Context<Self>) {
        if shown == self.shown {
            return;
        }
        self.shown = shown;
        self.region = None;
        self.from = None;
        if shown {
            window.resize(self.screen.size);
            window.set_input_region(None);
            window.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
            window.focus(&self.focus, cx);
            self.grown = Eased::lasting(0.0, DURATION_PANEL, now);
            self.grown.set_target(1.0, now);
            self.faded = Eased::lasting(0.0, DURATION_FADE, now);
            self.faded.set_target(1.0, now);
        } else {
            window.resize(size(RESTING, RESTING));
            window.set_input_region(Some(&[]));
            window.set_keyboard_interactivity(KeyboardInteractivity::None);
            set_blur_region(window, &[]);
            self.sent = None;
        }
    }

    fn with_capture(&self, cx: &mut Context<Self>, act: impl FnOnce(&mut Capture, &mut Context<Capture>)) {
        if let Some(capture) = self.capture.upgrade() {
            capture.update(cx, act);
        }
    }

    fn key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            self.with_capture(cx, |capture, cx| capture.cancel(cx));
            cx.stop_propagation();
        }
    }

    fn mode(&self, cx: &App) -> CaptureMode {
        self.capture.upgrade().map_or(CaptureMode::Region, |capture| capture.read(cx).mode())
    }

    fn press(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.from = Some(event.position);
        if self.mode(cx) == CaptureMode::Region {
            self.region = Some(region(event.position, event.position));
        }
        cx.notify();
    }

    fn cancel(&mut self, _event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.with_capture(cx, |capture, cx| capture.cancel(cx));
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
        // A press this picker didn't take (a toolbar button) is no pick.
        if self.from.take().is_none() {
            return;
        }
        let whole = bounds(point(px(0.0), px(0.0)), self.screen.size);
        let picked = match self.mode(cx) {
            CaptureMode::Screen => Some(whole),
            // A click isn't a region: the picker stays for another try.
            CaptureMode::Region => self.region.take().filter(|region| !is_click(*region, MIN_DRAG)),
        };
        if let Some(picked) = picked {
            self.take(picked, window, cx);
        }
        cx.notify();
    }

    /// Sends `picked` as the frame's pixels.
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

    fn ask(this: WeakEntity<Self>) -> impl Fn(Ask, &mut Window, &mut App) + Clone + 'static {
        move |ask, _, cx| {
            let _ = this.update(cx, |picker, cx| {
                picker.from = None;
                picker.with_capture(cx, |capture, cx| match ask {
                    Ask::Mode(mode) => capture.set_mode(mode, cx),
                    Ask::Close => capture.cancel(cx),
                });
            });
        }
    }

    /// Hands the toolbar's glass to the compositor when it moved: it blurs
    /// behind it.
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
        let picking = self.capture.upgrade().and_then(|capture| {
            let capture = capture.read(cx);
            capture.frame_for(&self.screen.name).cloned().map(|frame| (frame, capture.mode()))
        });
        self.show(picking.is_some(), now, window, cx);
        let Some((frame, mode)) = picking else {
            self.log_frame(false, now);
            return div().into_any_element();
        };
        if mode != self.seen_mode {
            self.seen_mode = mode;
            self.region = None;
        }
        let screen = self.screen.size;
        let layout = Layout::of(window);
        let (grown, shown) = (self.grown.value(now), self.faded.value(now));
        let glass = layout.bounds(screen.width, grown);
        self.blur(glass, window);
        let moving = !self.grown.settled(now) || !self.faded.settled(now);
        if moving {
            window.request_animation_frame();
        }
        self.log_frame(moving, now);

        // What the veil leaves clear: the region, or a hovered screen in
        // the Screen mode.
        let whole = bounds(point(px(0.0), px(0.0)), screen);
        let clear = match mode {
            CaptureMode::Screen if self.hovered => Some(whole),
            CaptureMode::Screen => None,
            CaptureMode::Region => self.region,
        };
        let theme = Theme::of(cx);
        let veil = hsla(0.0, 0.0, 0.0, VEIL_OPACITY);
        let pieces = around(clear.unwrap_or_default(), screen);
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
            .children(frame_around(&frame, glass, screen))
            .children(pieces.iter().filter(|piece| !piece.is_empty()).map(|piece| rect(*piece).bg(veil)));
        if let Some(clear) = clear.filter(|clear| !clear.is_empty()) {
            root = root.child(rect(clear.dilate(OUTLINE)).border(OUTLINE).border_color(theme.primary));
        }
        if let Some(region) = self.region.filter(|_| self.from.is_some()) {
            root = root.child(badge(&frame, region, screen, window, cx));
        }
        root.child(toolbar(&layout, &Look { glass, grown, shown, mode }, Self::ask(cx.weak_entity()), cx))
            .into_any_element()
    }
}

fn rect(at: Bounds<Pixels>) -> gpui::Div {
    div().absolute().left(at.origin.x).top(at.origin.y).w(at.size.width).h(at.size.height)
}

/// The frozen frame, but under the toolbar's glass: four pieces of it
/// around that.
fn frame_around(frame: &Frame, glass: Bounds<Pixels>, screen: gpui::Size<Pixels>) -> Vec<AnyElement> {
    around(glass, screen)
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

/// The region's size in the frame's pixels, under its bottom right corner
/// (inside it when that is off the screen), while it is dragged.
fn badge(frame: &Frame, region: Bounds<Pixels>, screen: gpui::Size<Pixels>, window: &Window, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let scale = if screen.width > px(0.0) { frame.pixels.width as f32 / f32::from(screen.width) } else { 1.0 };
    let pixels = to_pixels(region, scale, frame.pixels);
    let label = size_label(pixels.width, pixels.height);
    let mut face = gpui::font(FONT_MONO);
    face.weight = gpui::FontWeight::NORMAL;
    let run = gpui::TextRun {
        len: label.len(),
        font: face,
        color: theme.fg,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let text = window.text_system().shape_line(label.clone().into(), FONT_SIZE_SMALL, &[run], None).width;
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
