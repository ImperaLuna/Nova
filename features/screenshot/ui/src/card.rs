//! The card a saved screenshot lands in, in the bottom right corner of the
//! screen that had focus: the shot, that it was copied, where it went and
//! its size, and Open, Folder and Delete. A line along the bottom drains
//! over its time and the card leaves when it runs out; the pointer on the
//! card holds it. It slides in from the edge, and out before it goes.
//! `ui/capture/ShotCardWindow.qml` and `ShotCard.qml` in today's shell.
//!
//! Its own window on each screen, on the overlay layer like the volume
//! capsule, so it shows over fullscreen windows too: one pixel big while
//! no card is on its screen, else as big as the card plus its shadow,
//! with clicks and blur only on the card. It takes no keyboard and no
//! space, and asks for frames only while the card slides.

use crate::capture::{Capture, Card};
use crate::pick::{contain, folder_label, size_label};
use crate::tokens::{CARD_BUTTON_HEIGHT, CARD_GAP, CARD_WIDTH, DRAIN_HEIGHT, DRAIN_OPACITY, THUMB_HEIGHT};
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, CursorStyle, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, Render, RenderImage, StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window,
    WindowHandle, bounds, div, img,
    layer_shell::{Anchor, Layer},
    point, px, size,
};
use nova_look::tokens::{
    FONT_SIZE, FONT_SIZE_SMALL, GAP, ICON_GAP, ICON_SIZE, LABEL_NUDGE, LINE_HEIGHT, PAD, RADIUS_CONTROL, RADIUS_PANEL,
    RADIUS_ROW, SHADOW_ROOM, WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_look::{Glass, Glyph, Icon, LookStyled, Shape, ShellWindow, Theme, region, set_blur_region};
use nova_screenshot_api::{SavedScreenshot, ScreenshotAction};
use nova_stage::{FrameLog, Screen, Stage};
use std::sync::Arc;
use std::time::Instant;

/// The card's layer surfaces, for the compositor's rules (niri blurs
/// `^nova-`).
const NAMESPACE: &str = "nova-screenshot";
/// A window with no card: as small as the compositor takes.
const RESTING: Pixels = px(1.0);

const BUTTONS: [(ScreenshotAction, Glyph, &str); 3] = [
    (ScreenshotAction::Open, Glyph::OPEN, "Open"),
    (ScreenshotAction::Folder, Glyph::FOLDER, "Folder"),
    (ScreenshotAction::Delete, Glyph::TRASH, "Delete"),
];

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

/// The card's height: the shot, the two lines, the buttons.
fn card_height() -> Pixels {
    let content =
        THUMB_HEIGHT + CARD_GAP + title_height() + LABEL_NUDGE + detail_height() + CARD_GAP + CARD_BUTTON_HEIGHT;
    content + inset() * 2.0
}

/// The window, with the card at rest in it: its shadow's room on the left
/// and top, the card's gap from the screen's edges on the right and bottom.
fn window_size() -> gpui::Size<Pixels> {
    size(SHADOW_ROOM + CARD_WIDTH + CARD_GAP, SHADOW_ROOM + card_height() + CARD_GAP)
}

pub struct CardWindow {
    screen: Screen,
    capture: WeakEntity<Capture>,
    /// It is card sized now (what the window was last set to).
    shown: bool,
    /// The button under the pointer.
    hovered: Option<ScreenshotAction>,
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
                Self { screen, capture, shown: false, hovered: None, sent: None, frames, _followed: followed }
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

    fn show(&mut self, shown: bool, window: &mut Window) {
        if shown == self.shown {
            return;
        }
        self.shown = shown;
        self.hovered = None;
        if shown {
            window.resize(window_size());
        } else {
            window.resize(size(RESTING, RESTING));
            window.set_input_region(Some(&[]));
            set_blur_region(window, &[]);
            self.sent = None;
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
        self.show(card.is_some(), window);
        let Some(card) = card else {
            self.log_frame(false, now);
            return div().into_any_element();
        };
        let slide = card.slide.value(now);
        let moving = !card.slide.settled(now);
        let at = bounds(
            point(SHADOW_ROOM + (CARD_WIDTH + CARD_GAP) * (1.0 - slide), SHADOW_ROOM),
            size(CARD_WIDTH, card_height()),
        );
        let look = Look { card, slide, at, hovered: self.hovered, now };
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
    hovered: Option<ScreenshotAction>,
    now: Instant,
}

fn draw(look: &Look, this: WeakEntity<CardWindow>, capture: WeakEntity<Capture>, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let Look { card, slide, at, now, .. } = *look;
    // Sized here, in the middle of its room: GPUI lays a picture out by its
    // own shape and would crop it to the room's.
    let room = size(CARD_WIDTH - inset() * 2.0, THUMB_HEIGHT);
    let thumb = div()
        .flex_none()
        .w(room.width)
        .h(room.height)
        .flex()
        .items_center()
        .justify_center()
        .rounded(RADIUS_ROW)
        .bg(theme.backdrop)
        .overflow_hidden()
        .children(card.thumb.as_ref().map(|thumb| {
            let pixels = thumb.size(0);
            let fit = contain(size(pixels.width.0.max(0) as u32, pixels.height.0.max(0) as u32), room);
            img(Arc::<RenderImage>::clone(thumb)).flex_none().w(fit.width).h(fit.height)
        }));
    let held = capture.clone();
    let drain = div()
        .absolute()
        .left_0()
        .bottom_0()
        .h(DRAIN_HEIGHT)
        .w(CARD_WIDTH * card.countdown.remaining(now))
        .bg(theme.primary.opacity(DRAIN_OPACITY));
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
                .child(thumb)
                .child(labels(&card.shot, cx))
                .child(div().flex().gap(ICON_GAP).children(buttons(look.hovered, &this, &capture, cx))),
        )
        .child(drain)
        .into_any_element()
}

/// That it was copied (saved either way; copied unless the clipboard
/// failed), and where it went and its size.
fn labels(shot: &SavedScreenshot, cx: &App) -> impl IntoElement {
    let theme = Theme::of(cx);
    let title = if shot.copied { "Copied to clipboard" } else { "Saved" };
    let title = div()
        .flex()
        .items_center()
        .gap(ICON_GAP)
        .h(title_height())
        .font_weight(WEIGHT_TITLE)
        .child(Icon::new(Glyph::CHECK).color(theme.success))
        .child(title);
    let detail = format!("{} · {} · PNG", folder_label(&shot.path), size_label(shot.width, shot.height));
    let detail = div()
        .mt(LABEL_NUDGE)
        .h(detail_height())
        .text_size(FONT_SIZE_SMALL)
        .text_color(theme.fg_dim())
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .child(detail);
    div().flex().flex_col().child(title).child(detail)
}

/// Open, Folder and Delete, each as wide; Delete turns the error color
/// under the pointer.
fn buttons(
    hovered: Option<ScreenshotAction>,
    this: &WeakEntity<CardWindow>,
    capture: &WeakEntity<Capture>,
    cx: &App,
) -> Vec<AnyElement> {
    let theme = Theme::of(cx);
    BUTTONS
        .iter()
        .map(|(action, glyph, label)| {
            let action = *action;
            let lit = hovered == Some(action);
            let ink = if lit && action == ScreenshotAction::Delete { theme.error } else { theme.fg };
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
                        let now = if *hovered { Some(action) } else { window.hovered.filter(|was| *was != action) };
                        if now != window.hovered {
                            window.hovered = now;
                            cx.notify();
                        }
                    });
                })
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = capture.update(cx, |capture, cx| capture.act(action, cx));
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
        let window = window_size();
        assert_eq!(window.width, SHADOW_ROOM + CARD_WIDTH + CARD_GAP);
        assert!(card_height() > THUMB_HEIGHT + CARD_BUTTON_HEIGHT + CARD_GAP * 2.0);
        assert_eq!(window.height, SHADOW_ROOM + card_height() + CARD_GAP);
    }
}
