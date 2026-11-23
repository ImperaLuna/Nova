//! The card a saved screenshot or recording lands in, in the bottom right
//! corner of the screen that had focus: the shot (or the recording's
//! length), that it was saved, where it went, and Open, Folder and Delete.
//! A recording that didn't start, or ended without a frame, shows here
//! too: why, with Try again and Open log, for longer. A line along the
//! bottom drains over its time and the card leaves when it runs out; the
//! pointer on the card holds it. It slides in from the edge, and out
//! before it goes.
//! `ui/capture/ShotCardWindow.qml` and `ShotCard.qml` in today's shell.
//!
//! Its own window on each screen, on the overlay layer like the volume
//! capsule, so it shows over fullscreen windows too: one pixel big while
//! no card is on its screen, else as big as the card plus its shadow,
//! with clicks and blur only on the card. It takes no keyboard and no
//! space, and asks for frames only while the card slides.

use crate::capture::{Button, Capture, Card, Saved};
use crate::pick::{clock, contain, folder_label, size_label};
use crate::tokens::{
    CARD_BUTTON_HEIGHT, CARD_GAP, CARD_WIDTH, DRAIN_HEIGHT, DRAIN_OPACITY, FAILED_CARD_LINES, THUMB_HEIGHT,
};
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, CursorStyle, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, Render, RenderImage, SharedString, StatefulInteractiveElement, Styled, Subscription,
    TextRun, WeakEntity, Window, WindowHandle, bounds, div, font, img,
    layer_shell::{Anchor, Layer},
    point, px, size,
};
use nova_look::tokens::{
    FONT_MONO, FONT_SANS, FONT_SIZE, FONT_SIZE_SMALL, GAP, ICON_GAP, ICON_SIZE, LABEL_NUDGE, LINE_HEIGHT, PAD,
    RADIUS_CONTROL, RADIUS_PANEL, RADIUS_ROW, SHADOW_ROOM, WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_look::{Glass, Glyph, Icon, LookStyled, Shape, ShellWindow, Theme, region, set_blur_region};
use nova_screenshot_api::ScreenshotAction;
use nova_stage::{FrameLog, Screen, Stage};
use std::sync::Arc;
use std::time::Instant;

/// The card's layer surfaces, for the compositor's rules (niri blurs
/// `^nova-`).
const NAMESPACE: &str = "nova-screenshot";
/// A window with no card: as small as the compositor takes.
const RESTING: Pixels = px(1.0);

const FILE_BUTTONS: [(Button, Glyph, &str); 3] = [
    (Button::File(ScreenshotAction::Open), Glyph::OPEN, "Open"),
    (Button::File(ScreenshotAction::Folder), Glyph::FOLDER, "Folder"),
    (Button::File(ScreenshotAction::Delete), Glyph::TRASH, "Delete"),
];
const FAILED_BUTTONS: [(Button, Glyph, &str); 2] =
    [(Button::Retry, Glyph::REBOOT, "Try again"), (Button::Log, Glyph::DOCUMENT, "Open log")];
const DELETE: Button = Button::File(ScreenshotAction::Delete);

/// How tall the title line and the line under it are.
fn title_height() -> Pixels {
    (FONT_SIZE * LINE_HEIGHT).max(ICON_SIZE).ceil()
}

fn detail_height() -> Pixels {
    (FONT_SIZE_SMALL * LINE_HEIGHT).ceil()
}

/// Between the card's edge and what is on it.
fn inset() -> Pixels {
    PAD + GAP / 2.0
}

/// The card's height: the shot, the two lines and the buttons; or, for a
/// recording that failed, the title, the reason's `lines` and the
/// buttons.
fn card_height(failed: bool, lines: usize) -> Pixels {
    let content = if failed {
        title_height() + LABEL_NUDGE + detail_height() * lines as f32 + CARD_GAP + CARD_BUTTON_HEIGHT
    } else {
        THUMB_HEIGHT + CARD_GAP + title_height() + LABEL_NUDGE + detail_height() + CARD_GAP + CARD_BUTTON_HEIGHT
    };
    content + inset() * 2.0
}

/// The window, with a card `height` tall at rest in it: its shadow's room
/// on the left and top, the card's gap from the screen's edges on the
/// right and bottom.
fn window_size(height: Pixels) -> gpui::Size<Pixels> {
    size(SHADOW_ROOM + CARD_WIDTH + CARD_GAP, SHADOW_ROOM + height + CARD_GAP)
}

/// The lines novad's reason takes on the card, at most
/// [`FAILED_CARD_LINES`].
fn reason_lines(reason: &str, window: &Window) -> usize {
    let run = TextRun {
        len: reason.len(),
        font: font(FONT_SANS),
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let width = CARD_WIDTH - inset() * 2.0;
    let shaped = window.text_system().shape_text(
        SharedString::from(reason.to_owned()),
        FONT_SIZE_SMALL,
        &[run],
        Some(width),
        None,
    );
    let lines = shaped.map_or(1, |lines| lines.iter().map(|line| line.wrap_boundaries().len() + 1).sum());
    lines.clamp(1, FAILED_CARD_LINES)
}

pub struct CardWindow {
    screen: Screen,
    capture: WeakEntity<Capture>,
    /// The window's size now: card sized, or none while it rests.
    sized: Option<gpui::Size<Pixels>>,
    /// The button under the pointer.
    hovered: Option<Button>,
    /// The card's place as the compositor last got it.
    sent: Option<Bounds<Pixels>>,
    frames: Option<FrameLog>,
    _followed: Option<Subscription>,
}

impl CardWindow {
    pub fn open(screen: &Screen, capture: WeakEntity<Capture>, cx: &mut App) -> Option<WindowHandle<Self>> {
        let mut shell = ShellWindow::new(screen.id, size(RESTING, RESTING));
        shell.layer = Layer::Overlay;
        shell.anchor = Anchor::BOTTOM | Anchor::RIGHT;
        shell.namespace = NAMESPACE;
        let screen = screen.clone();
        let frames = Stage::logs_frames(cx)
            .then(|| FrameLog::new(format!("{}/screenshot", screen.name), screen.refresh, Instant::now()));
        let opened = cx.open_window(shell.options(), move |window, cx| {
            window.set_input_region(Some(&[]));
            cx.new(|cx: &mut Context<Self>| {
                let followed = capture.upgrade().map(|capture| cx.observe(&capture, |_, _, cx| cx.notify()));
                Self { screen, capture, sized: None, hovered: None, sent: None, frames, _followed: followed }
            })
        });
        match opened {
            Ok(window) => Some(window),
            Err(error) => {
                eprintln!("ERROR nova-ui: can't open the screenshot card's window: {error:#}");
                None
            }
        }
    }

    pub fn set_screen(&mut self, screen: &Screen, cx: &mut Context<Self>) {
        if self.screen.size != screen.size {
            self.screen = screen.clone();
            cx.notify();
        }
    }

    /// Sizes the window for a card `height` tall, or rests it.
    fn show(&mut self, height: Option<Pixels>, window: &mut Window) {
        let sized = height.map(window_size);
        if sized == self.sized {
            return;
        }
        if self.sized.is_none() {
            self.hovered = None;
        }
        self.sized = sized;
        match sized {
            Some(sized) => window.resize(sized),
            None => {
                window.resize(size(RESTING, RESTING));
                window.set_input_region(Some(&[]));
                set_blur_region(window, &[]);
                self.sent = None;
            }
        }
    }

    /// Hands the card's place to the compositor when it moved: it blurs
    /// behind it, and clicks land on it alone.
    fn regions(&mut self, card: Bounds<Pixels>, window: &Window) {
        if self.sent != Some(card) {
            let shapes = [Shape::new(card, RADIUS_PANEL)];
            set_blur_region(window, &shapes);
            window.set_input_region(Some(&region(&shapes)));
            self.sent = Some(card);
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

impl Render for CardWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let Some(capture) = self.capture.upgrade() else { return div().into_any_element() };
        capture.update(cx, |capture, cx| capture.step(now, cx));
        let capture_now = capture.read(cx);
        let card = capture_now.card().filter(|card| card.screen == self.screen.id);
        let lines = match card.map(|card| &card.saved) {
            Some(Saved::Failed { reason, .. }) => reason_lines(reason, window),
            _ => 1,
        };
        let height = card.map(|card| card_height(matches!(card.saved, Saved::Failed { .. }), lines));
        self.show(height, window);
        let (Some(card), Some(height)) = (card, height) else {
            self.log_frame(false, now);
            return div().into_any_element();
        };
        let slide = card.slide.value(now);
        let moving = !card.slide.settled(now);
        let at =
            bounds(point(SHADOW_ROOM + (CARD_WIDTH + CARD_GAP) * (1.0 - slide), SHADOW_ROOM), size(CARD_WIDTH, height));
        let look = Look { card, slide, at, lines, hovered: self.hovered, now };
        let drawn = draw(&look, cx.weak_entity(), self.capture.clone(), cx);
        self.regions(at, window);
        if moving {
            window.request_animation_frame();
        }
        self.log_frame(moving, now);
        div().size_full().body_text(cx).child(drawn).into_any_element()
    }
}

/// How the card is drawn this frame: slid in to `slide`, at `at`, with
/// the button under the pointer lit.
struct Look<'a> {
    card: &'a Card,
    slide: f32,
    at: Bounds<Pixels>,
    /// The lines a failed recording's reason takes.
    lines: usize,
    hovered: Option<Button>,
    now: Instant,
}

fn draw(look: &Look, this: WeakEntity<CardWindow>, capture: WeakEntity<Capture>, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let Look { card, slide, at, now, .. } = *look;
    let failed = matches!(card.saved, Saved::Failed { .. });
    let held = capture.clone();
    let drain_color = if failed { theme.error } else { theme.primary };
    let drain = div()
        .absolute()
        .left_0()
        .bottom_0()
        .h(DRAIN_HEIGHT)
        .w(CARD_WIDTH * card.countdown.remaining(now))
        .bg(drain_color.opacity(DRAIN_OPACITY));
    let buttons: &[(Button, Glyph, &str)] = if failed { &FAILED_BUTTONS } else { &FILE_BUTTONS };
    Glass::new(RADIUS_PANEL)
        .shadow(slide)
        .build(cx)
        .id("card")
        .absolute()
        .left(at.origin.x)
        .top(at.origin.y)
        .w(at.size.width)
        .h(at.size.height)
        .opacity(slide)
        .on_hover(move |hovered, _, cx| {
            let _ = held.update(cx, |capture, cx| capture.hold(*hovered, cx));
        })
        .child(
            div()
                .absolute()
                .inset(inset())
                .flex()
                .flex_col()
                .gap(CARD_GAP)
                .children((!failed).then(|| thumb(card, cx)))
                .child(labels(&card.saved, look.lines, cx))
                .child(div().flex().gap(ICON_GAP).children(draw_buttons(buttons, look.hovered, &this, &capture, cx))),
        )
        .child(drain)
        .into_any_element()
}

/// The shot, fitted whole; or for a recording, a video icon over its
/// length.
fn thumb(card: &Card, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    // Sized here, in the middle of its room: GPUI lays a picture out by its
    // own shape and would crop it to the room's.
    let room = size(CARD_WIDTH - inset() * 2.0, THUMB_HEIGHT);
    let thumb = div()
        .flex_none()
        .w(room.width)
        .h(room.height)
        .flex()
        .flex_col()
        .gap(ICON_GAP)
        .items_center()
        .justify_center()
        .rounded(RADIUS_ROW)
        .bg(theme.backdrop)
        .overflow_hidden();
    match &card.saved {
        Saved::Recording(saved) => thumb
            .child(Icon::new(Glyph::VIDEO).color(theme.fg_dim()))
            .child(div().font_family(FONT_MONO).text_color(theme.fg).child(clock(saved.duration_ms))),
        _ => thumb.children(card.thumb.as_ref().map(|thumb| {
            let pixels = thumb.size(0);
            let fit = contain(size(pixels.width.0.max(0) as u32, pixels.height.0.max(0) as u32), room);
            img(Arc::<RenderImage>::clone(thumb)).flex_none().w(fit.width).h(fit.height)
        })),
    }
    .into_any_element()
}

/// What happened (saved, copied unless the clipboard failed, didn't
/// start or recorded nothing), and where it went with its size or length, or why it didn't.
fn labels(saved: &Saved, lines: usize, cx: &App) -> impl IntoElement {
    let theme = Theme::of(cx);
    let (glyph, ink, title) = match saved {
        Saved::Shot(shot) if shot.copied => (Glyph::CHECK, theme.success, "Copied to clipboard"),
        Saved::Shot(_) => (Glyph::CHECK, theme.success, "Saved"),
        Saved::Recording(_) => (Glyph::CHECK, theme.success, "Recording saved"),
        Saved::Failed { started: false, .. } => (Glyph::VIDEO_OFF, theme.error, "Recording didn't start"),
        Saved::Failed { started: true, .. } => (Glyph::VIDEO_OFF, theme.error, "Recording not saved"),
    };
    let title = div()
        .flex()
        .items_center()
        .gap(ICON_GAP)
        .h(title_height())
        .font_weight(WEIGHT_TITLE)
        .child(Icon::new(glyph).color(ink))
        .child(title);
    let detail = div().mt(LABEL_NUDGE).text_size(FONT_SIZE_SMALL).text_color(theme.fg_dim());
    let detail = match saved {
        Saved::Failed { reason, .. } => {
            detail.h(detail_height() * lines as f32).line_clamp(lines).child(reason.clone())
        }
        Saved::Shot(shot) => {
            let line = format!("{} · {} · PNG", folder_label(&shot.path), size_label(shot.width, shot.height));
            detail.h(detail_height()).whitespace_nowrap().overflow_hidden().text_ellipsis().child(line)
        }
        Saved::Recording(saved) => {
            let line = format!("{} · {} · MP4", folder_label(&saved.path), clock(saved.duration_ms));
            detail.h(detail_height()).whitespace_nowrap().overflow_hidden().text_ellipsis().child(line)
        }
    };
    div().flex().flex_col().child(title).child(detail)
}

/// The card's buttons, each as wide; Delete turns the error color under
/// the pointer.
fn draw_buttons(
    buttons: &[(Button, Glyph, &'static str)],
    hovered: Option<Button>,
    this: &WeakEntity<CardWindow>,
    capture: &WeakEntity<Capture>,
    cx: &App,
) -> Vec<AnyElement> {
    let theme = Theme::of(cx);
    buttons
        .iter()
        .map(|(button, glyph, label)| {
            let button = *button;
            let lit = hovered == Some(button);
            let ink = if lit && button == DELETE { theme.error } else { theme.fg };
            let (this, capture) = (this.clone(), capture.clone());
            div()
                .id(*label)
                .flex_1()
                .h(CARD_BUTTON_HEIGHT)
                .flex()
                .items_center()
                .justify_center()
                .gap(ICON_GAP)
                .rounded(RADIUS_CONTROL)
                .bg(if lit { theme.hover() } else { theme.surface_raised() })
                .cursor(CursorStyle::PointingHand)
                .font_weight(WEIGHT_LABEL)
                .text_color(ink)
                .child(Icon::new(*glyph).color(ink))
                .child(*label)
                .on_hover(move |hovered, _, cx| {
                    let _ = this.update(cx, |window, cx| {
                        let now = if *hovered { Some(button) } else { window.hovered.filter(|was| *was != button) };
                        if now != window.hovered {
                            window.hovered = now;
                            cx.notify();
                        }
                    });
                })
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = capture.update(cx, |capture, cx| capture.act(button, cx));
                })
                .into_any_element()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_holds_the_card_its_gaps_and_its_shadow() {
        let height = card_height(false, 1);
        let window = window_size(height);
        assert_eq!(window.width, SHADOW_ROOM + CARD_WIDTH + CARD_GAP);
        assert!(height > THUMB_HEIGHT + CARD_BUTTON_HEIGHT + CARD_GAP * 2.0);
        assert_eq!(window.height, SHADOW_ROOM + height + CARD_GAP);
    }

    #[test]
    fn a_failed_recording_s_card_grows_with_its_reason() {
        assert!(card_height(true, 1) < card_height(false, 1));
        assert_eq!(card_height(true, 3) - card_height(true, 1), detail_height() * 2.0);
    }
}
