//! One screen's capsule window: on the overlay layer against the right edge,
//! vertically centered, so it shows over fullscreen video too; taking no
//! keyboard and no space, with clicks and blur only on the capsule. While
//! nothing is out it rests a pixel big and draws nothing; it grows before the
//! capsule comes out, and shrinks once it's gone, since a resize in a motion
//! costs that motion frames. While out it draws every frame: the liquid never
//! stops moving.
//!
//! The capsule's glass, liquid and shadow are one shader ([`crate::shader`]);
//! the icon and number lie over it, drawn twice, in the glass's ink above
//! the liquid's surface and the liquid's ink below it, each copy clipped at
//! the surface. With no output a dashed outline replaces the lit rim.
//! Scrolling on it steps the volume, dragging or clicking sets it, and
//! clicking the icon or number mutes.

use crate::capsule::Capsule;
use crate::logic::{self, CapsuleBox};
use crate::shader::{self, Look};
use crate::tokens::{CORNER_POINTS, DRAG_DISTANCE, GROW_WAIT, LABEL_PAD, RESTING, RIM_GLOW_OPACITY};
use gpui::{
    App, AppContext, Bounds, Context, CursorStyle, Div, DragMoveEvent, Entity, Hsla, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, ParentElement, PathBuilder, Pixels, Point, Render, ScrollEvent,
    StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, WindowHandle, canvas, div,
    layer_shell::{Anchor, Layer},
    point, px, size,
};
use nova_look::tokens::{
    DIM_OPACITY, FONT_MONO, FONT_SIZE, LABEL_NUDGE, RING_DASH, SHADOW_ROOM, STROKE_THIN, WEIGHT_TITLE,
};
use nova_look::{LookStyled, Shape, ShellWindow, Theme, VolumeGlyph, Wheel, region, set_blur_region};
use nova_stage::{FrameLog, Screen, Stage};
use std::cell::{Cell, RefCell};
use std::f32::consts::FRAC_PI_2;
use std::rc::Rc;
use std::time::Instant;

/// The capsule's layer surfaces, for the compositor's rules (niri blurs
/// `^nova-`).
const NAMESPACE: &str = "nova-volume";

/// The window's size with the capsule out: as big as it gets plus its
/// shadow.
fn full_size() -> gpui::Size<Pixels> {
    let height = (logic::tallest() + 2.0 * f32::from(SHADOW_ROOM)).ceil();
    size(px(logic::widest()) + SHADOW_ROOM, px(height))
}

pub struct CapsuleWindow {
    screen: Screen,
    capsule: WeakEntity<Capsule>,
    /// It's out, or on its way, on this screen, as the capsule last said.
    shows: bool,
    /// The size it last asked for, and when.
    size: gpui::Size<Pixels>,
    resized: Option<Instant>,
    /// Where the capsule was drawn last frame, in the window.
    drawn: Option<CapsuleBox>,
    /// The shapes the compositor has: blur and clicks.
    sent: Rc<RefCell<Option<Vec<Shape>>>>,
    /// Where the icon and number were laid out, for a click on them.
    face: Rc<Cell<Bounds<Pixels>>>,
    /// A press on it, and whether it moved far enough to be a drag.
    pressed: Option<Pixels>,
    dragged: bool,
    hovered: bool,
    wheel: Wheel,
    frames: Option<FrameLog>,
    _followed: Option<Subscription>,
}

/// What a drag on the capsule carries.
struct Dragging;

/// A drag's picture: nothing, the liquid follows the pointer instead.
struct NoPicture;

impl Render for NoPicture {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl CapsuleWindow {
    pub fn open(screen: &Screen, capsule: WeakEntity<Capsule>, cx: &mut App) -> Option<WindowHandle<Self>> {
        let mut shell = ShellWindow::new(screen.id, size(RESTING, RESTING));
        shell.layer = Layer::Overlay;
        shell.anchor = Anchor::RIGHT;
        shell.namespace = NAMESPACE;
        let screen = screen.clone();
        let frames = Stage::logs_frames(cx)
            .then(|| FrameLog::new(format!("{}/volume", screen.name), screen.refresh, Instant::now()));
        let opened = cx.open_window(shell.options(), move |_, cx| {
            cx.new(|cx: &mut Context<Self>| {
                // Whether there's anything to draw is found out here, not
                // while drawing: GPUI redraws a window whenever something its
                // last frame read changes, so a window that read the capsule
                // to learn it isn't out would commit a frame, on every
                // screen, each time the volume changes.
                let followed = capsule.upgrade().map(|capsule| cx.observe(&capsule, Self::follow));
                let shows = capsule.upgrade().is_some_and(|capsule| capsule.read(cx).shows_on(screen.id));
                Self {
                    screen,
                    capsule,
                    shows,
                    size: size(RESTING, RESTING),
                    resized: None,
                    drawn: None,
                    sent: Rc::default(),
                    face: Rc::default(),
                    pressed: None,
                    dragged: false,
                    hovered: false,
                    wheel: Wheel::default(),
                    frames,
                    _followed: followed,
                }
            })
        });
        match opened {
            Ok(window) => Some(window),
            Err(error) => {
                eprintln!("ERROR nova-ui: can't open the volume capsule's window: {error:#}");
                None
            }
        }
    }

    /// The capsule changed: draws when it's out here, and once more when
    /// it no longer is.
    fn follow(&mut self, capsule: Entity<Capsule>, cx: &mut Context<Self>) {
        let shows = capsule.read(cx).shows_on(self.screen.id);
        if shows || self.shows {
            self.shows = shows;
            cx.notify();
        }
    }

    /// Makes the window `wanted` big. Only for a frame in which nothing
    /// moves.
    fn fit(&mut self, wanted: gpui::Size<Pixels>, now: Instant, window: &mut Window) {
        if wanted == self.size {
            return;
        }
        self.size = wanted;
        self.resized = Some(now);
        window.resize(wanted);
    }

    /// The window has the size it last asked for, or the compositor has
    /// had long enough to give it and chose another.
    fn fitted(&self, now: Instant, window: &Window) -> bool {
        window.viewport_size() == self.size
            || self.resized.is_none_or(|at| now.saturating_duration_since(at) > GROW_WAIT)
    }

    /// Nothing to draw: as small as it gets, with no shapes.
    fn rest(&mut self, now: Instant, window: &mut Window, cx: &mut Context<Self>) -> Div {
        self.fit(size(RESTING, RESTING), now, window);
        self.drawn = None;
        self.let_go(cx);
        self.log_frame(false, now);
        div().child(self.regions(Vec::new()))
    }

    /// Hands this frame's shapes to the compositor when they changed: it
    /// blurs behind them, and clicks land on them alone.
    fn regions(&self, shapes: Vec<Shape>) -> impl IntoElement {
        let sent = self.sent.clone();
        canvas(
            |_, _, _| {},
            move |_, (), window, _| {
                if sent.borrow().as_ref() != Some(&shapes) {
                    set_blur_region(window, &shapes);
                    window.set_input_region(Some(&region(&shapes)));
                    sent.replace(Some(shapes));
                }
            },
        )
        .absolute()
        .size_0()
    }

    fn log_frame(&mut self, moving: bool, now: Instant) {
        if let Some(frames) = &mut self.frames {
            for line in frames.frame(moving, now) {
                eprintln!("INFO nova-ui: {line}");
            }
        }
    }

    fn update_capsule(&self, cx: &mut App, update: impl FnOnce(&mut Capsule, &mut Context<Capsule>)) {
        if let Some(capsule) = self.capsule.upgrade() {
            capsule.update(cx, update);
        }
    }

    /// Whether the pointer holds it out: on it, or dragging.
    fn sync_held(&self, cx: &mut App) {
        let held = self.hovered || self.pressed.is_some();
        self.update_capsule(cx, |capsule, cx| capsule.set_held(held, cx));
    }

    /// The pointer is gone from it (it went in, or its screen left).
    fn let_go(&mut self, cx: &mut App) {
        if self.hovered || self.pressed.is_some() {
            self.hovered = false;
            self.pressed = None;
            self.sync_held(cx);
        }
    }

    fn hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.hovered = hovered;
        self.sync_held(cx);
    }

    fn scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let notches = self.wheel.notches(event);
        if notches != 0 {
            self.update_capsule(cx, |capsule, cx| capsule.scroll(notches, cx));
        }
    }

    fn press(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.pressed = Some(event.position.y);
        self.dragged = false;
        self.sync_held(cx);
    }

    /// The volume at the pointer's height `y` in the window.
    fn volume_at(&self, y: Pixels) -> Option<f32> {
        let drawn = self.drawn?;
        Some(logic::volume_at(f32::from(y) - drawn.y, drawn.h))
    }

    fn drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(pressed) = self.pressed else { return };
        if (position.y - pressed).abs() > DRAG_DISTANCE {
            self.dragged = true;
        }
        if self.dragged
            && let Some(volume) = self.volume_at(position.y)
        {
            self.update_capsule(cx, |capsule, cx| capsule.set_volume(volume, cx));
        }
    }

    /// A press let go on it: a click on the icon or number mutes, anywhere
    /// else sets the volume there; the end of a drag does nothing more.
    fn release(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let clicked = self.pressed.take().is_some() && !self.dragged;
        self.dragged = false;
        if clicked {
            let face = self.face.get();
            if position.y >= face.top() && position.y <= face.bottom() {
                self.update_capsule(cx, |capsule, cx| capsule.toggle_mute(cx));
            } else if let Some(volume) = self.volume_at(position.y) {
                self.update_capsule(cx, |capsule, cx| capsule.set_volume(volume, cx));
            }
        }
        self.sync_held(cx);
    }

    fn release_outside(&mut self, cx: &mut Context<Self>) {
        self.pressed = None;
        self.dragged = false;
        self.sync_held(cx);
    }
}

impl Render for CapsuleWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let Some(capsule) = self.capsule.upgrade().filter(|_| self.shows) else { return self.rest(now, window, cx) };
        // It grows first, and the capsule comes out in the frame after it has.
        if capsule.read(cx).wants_out() {
            self.fit(full_size(), now, window);
            if self.fitted(now, window) {
                capsule.update(cx, |capsule, _| capsule.go_out());
            }
            window.request_animation_frame();
        }
        capsule.update(cx, |capsule, _| capsule.step(now));
        let capsule_now = capsule.read(cx);
        if !capsule_now.shows_on(self.screen.id) {
            // It went in this frame, which read the capsule: one more,
            // which reads nothing, lets go of it.
            self.shows = false;
            window.request_animation_frame();
            return self.rest(now, window, cx);
        }
        let motion = capsule_now.motion().clone();
        let output = capsule_now.output();
        let (swell, muted) = (capsule_now.swell(now), capsule_now.muted(now));
        let viewport = window.viewport_size();
        let drawn = logic::capsule_box(
            motion.open.value,
            motion.open.speed,
            f32::from(viewport.width),
            f32::from(viewport.height),
        );
        self.drawn = Some(drawn);
        let look = Look { capsule: drawn, swell, muted, rim: output.is_some() };
        let bounds = Bounds::new(point(px(drawn.x), px(drawn.y)), size(px(drawn.w), px(drawn.h)));
        let shapes = vec![Shape::new(bounds, px(drawn.radius))];
        let theme = Theme::of(cx).clone();
        let paint_motion = motion.clone();
        let glass = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| shader::paint(bounds, &look, &paint_motion, &theme, window),
        )
        .absolute()
        .inset_0();

        // The window is never resized while it's out, and it is out the
        // whole time it draws: every frame.
        window.request_animation_frame();
        self.log_frame(true, now);
        let surface = logic::surface_y(motion.level.value, drawn.h);
        let hit = div()
            .id("capsule")
            .absolute()
            .left(px(drawn.x))
            .top(px(drawn.y))
            .w(px(drawn.w))
            .h(px(drawn.h))
            .when_some(output, |hit, _| {
                hit.cursor(CursorStyle::ResizeUpDown)
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.hover(*hovered, cx)))
                    .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| this.scroll(event, cx)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| this.press(event, cx)),
                    )
                    .on_drag(Dragging, |_, _, _, cx| cx.new(|_| NoPicture))
                    .on_drag_move(cx.listener(|this, event: &DragMoveEvent<Dragging>, _, cx| {
                        this.drag(event.event.position, cx);
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                            this.release(event.position, cx);
                        }),
                    )
                    .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.release_outside(cx)))
            })
            .when(output.is_none(), |hit| hit.child(dashed_outline(drawn, cx)))
            .child(label(LabelAt { output, open: motion.open.value, height: drawn.h, surface }, &self.face, cx));
        div().size_full().body_text(cx).child(glass).child(hit).child(self.regions(shapes))
    }
}

use gpui::prelude::FluentBuilder;

/// What the label shows and where: the output in use, how far the capsule
/// is out, its height and the liquid's surface (from its top).
struct LabelAt {
    output: Option<logic::Output>,
    open: f32,
    height: f32,
    surface: f32,
}

/// The icon and number at the bottom of the capsule, readable on glass and
/// on liquid alike.
fn label(at: LabelAt, face: &Rc<Cell<Bounds<Pixels>>>, cx: &App) -> impl IntoElement {
    let LabelAt { output, open, height, surface } = at;
    let theme = Theme::of(cx);
    let glyph = VolumeGlyph::pick(
        output.map(|output| output.kind),
        output.is_some_and(|o| o.muted),
        output.map_or(0.0, |o| o.volume),
    );
    let text = logic::label(output);
    let dimmed = output.is_some_and(|output| output.muted);
    let split = surface.clamp(0.0, height);
    // One copy in `ink`, laid out in the whole label's box, `top` above the
    // clip that holds it.
    let copy = |ink: Hsla, top: f32, measured: Option<Rc<Cell<Bounds<Pixels>>>>| {
        let column = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(LABEL_NUDGE)
            .child(glyph.icon().color(ink))
            .child(
                div()
                    .font_family(FONT_MONO)
                    .text_size(FONT_SIZE)
                    .font_weight(WEIGHT_TITLE)
                    .text_color(ink)
                    .when(dimmed, |number| number.opacity(DIM_OPACITY))
                    .child(text.clone()),
            )
            .when_some(measured, |column, face| {
                column.child(canvas(move |bounds, _, _| face.set(bounds), |_, (), _, _| {}).absolute().inset_0())
            });
        div()
            .absolute()
            .left_0()
            .top(px(-top))
            .w_full()
            .h(px(height))
            .flex()
            .flex_col()
            .justify_end()
            .items_center()
            .pb(LABEL_PAD)
            .child(column.relative())
    };
    div()
        .absolute()
        .inset_0()
        .opacity(logic::label_opacity(open))
        .child(div().absolute().left_0().top_0().w_full().h(px(split)).overflow_hidden().child(copy(
            theme.fg,
            0.0,
            Some(face.clone()),
        )))
        .child(div().absolute().left_0().top(px(split)).w_full().h(px(height - split)).overflow_hidden().child(copy(
            theme.on_accent,
            split,
            None,
        )))
}

/// No output: a dashed outline in place of the lit rim.
fn dashed_outline(drawn: CapsuleBox, cx: &App) -> impl IntoElement {
    let color = Theme::of(cx).fg.alpha(RIM_GLOW_OPACITY);
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let inset = f32::from(STROKE_THIN) / 2.0;
            let points = rounded_rect(bounds, inset, drawn.radius - inset);
            let mut path = PathBuilder::stroke(STROKE_THIN).dash_array(&[px(RING_DASH), px(RING_DASH)]);
            path.add_polygon(&points, true);
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .absolute()
    .inset_0()
}

/// The outline of `bounds` inset by `inset`, with corners of `radius`, as
/// points clockwise from the top left corner's end.
fn rounded_rect(bounds: Bounds<Pixels>, inset: f32, radius: f32) -> Vec<Point<Pixels>> {
    let (left, top) = (f32::from(bounds.left()) + inset, f32::from(bounds.top()) + inset);
    let (right, bottom) = (f32::from(bounds.right()) - inset, f32::from(bounds.bottom()) - inset);
    let radius = radius.min((right - left) / 2.0).min((bottom - top) / 2.0).max(0.0);
    // Each corner's center and the angle its arc starts at (y down).
    let corners = [
        (right - radius, top + radius, -FRAC_PI_2),
        (right - radius, bottom - radius, 0.0),
        (left + radius, bottom - radius, FRAC_PI_2),
        (left + radius, top + radius, 2.0 * FRAC_PI_2),
    ];
    let mut points = Vec::with_capacity(corners.len() * (CORNER_POINTS + 1));
    for (x, y, start) in corners {
        for step in 0..=CORNER_POINTS {
            let angle = start + FRAC_PI_2 * step as f32 / CORNER_POINTS as f32;
            points.push(point(px(x + radius * angle.cos()), px(y + radius * angle.sin())));
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::{EDGE_GAP, WIDTH};

    #[test]
    fn the_window_holds_the_tallest_capsule_and_its_shadow() {
        let full = full_size();
        assert!(f32::from(full.height) >= logic::tallest() + 2.0 * f32::from(SHADOW_ROOM));
        assert!(full.width > WIDTH + EDGE_GAP + SHADOW_ROOM, "room for the slide's overshoot");
    }

    #[test]
    fn the_outline_follows_the_capsule_s_round_ends() {
        let bounds = Bounds::new(point(px(10.0), px(20.0)), size(px(48.0), px(260.0)));
        let points = rounded_rect(bounds, 0.0, 24.0);
        for corner in &points {
            let (x, y) = (f32::from(corner.x), f32::from(corner.y));
            assert!((10.0..=58.0 + 1e-3).contains(&x) && (20.0 - 1e-3..=280.0 + 1e-3).contains(&y), "{x} {y}");
        }
        // The top end is a half circle: its middle point is the top.
        assert!(points.iter().any(|p| (f32::from(p.y) - 20.0).abs() < 1e-3 && (f32::from(p.x) - 34.0).abs() < 1e-3));
    }
}
