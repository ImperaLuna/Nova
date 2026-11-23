//! Drawing a screen's stage. Back to front: the panels that are open or on
//! their way, then the bar's three rows of pills, then (drawing nothing)
//! the regions.
//!
//! A panel's glass is the box between its pill and its open place, so the
//! pill seems to grow: the pill's own glass steps aside the moment its
//! panel starts, its content fades out, and the panel's content, laid out
//! at its open size and clipped to the glass, fades in once there is room.
//! A pill an open panel lies over fades away, since glass is see-through.
//!
//! No feature tells the compositor where anything is. Every glass records
//! its box while the frame is laid out, and the frame's last element hands
//! the list to the compositor as the window's blur and click regions, only
//! when it differs from the one sent before. Frames are asked for only
//! while something here is on its way; a view that changes by itself (a
//! clock) redraws through GPUI like any view.

use crate::geometry::{Align, between, covered, open_box, overlaps, radius_between};
use crate::stage::Stage;
use crate::tokens::{PANEL_ENTER_RISE, PILL_FADE_OUT};
use crate::view::{PanelItem, PillBox, PillItem, ROWS, StageView};
use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, Div, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, ParentElement, Pixels, Render, ScrollEvent, Styled, Window, canvas, div, point, px, size,
};
use nova_look::tokens::{BAR_GAP, BAR_HEIGHT, BAR_MARGIN, GAP};
use nova_look::{Glass, LookStyled, Shape, region, set_blur_region};
use std::rc::Rc;
use std::time::Instant;

/// The key that closes panels, as GPUI names it.
const ESCAPE: &str = "escape";

/// Where a panel is drawn right now.
struct PanelFrame {
    glass: Bounds<Pixels>,
    radius: Pixels,
    /// Its place once open.
    open: Bounds<Pixels>,
    /// The whole panel's opacity: below 1 only for one rising into place.
    opacity: f32,
    /// Where its content sits inside the glass.
    content_at: gpui::Point<Pixels>,
}

impl StageView {
    /// Where `panel` is at `now`.
    fn panel_frame(&self, panel: &PanelItem, now: Instant) -> PanelFrame {
        let progress = panel.morph.value(now);
        let open_size = size(px(panel.width.value(now)), px(panel.height.value(now)));
        let from = panel.from.as_ref().and_then(|view| self.pill_box(view));
        let open = open_box(from, open_size, panel.spec.top, self.screen.size.width);
        match from {
            Some((pill, _)) => {
                let glass = between(pill, open, progress);
                PanelFrame {
                    glass,
                    radius: radius_between(panel.spec.radius, progress),
                    open,
                    opacity: 1.0,
                    // The content stays where it will be; the glass grows over it.
                    content_at: point(open.left() - glass.left(), open.top() - glass.top()),
                }
            }
            None => {
                let risen = PANEL_ENTER_RISE * (1.0 - progress);
                let glass = Bounds { origin: point(open.left(), open.top() - risen), size: open.size };
                PanelFrame {
                    glass,
                    radius: panel.spec.radius,
                    open,
                    opacity: progress,
                    content_at: point(px(0.0), px(0.0)),
                }
            }
        }
    }

    /// Hides each pill an open panel lies over, and shows the others.
    fn give_way(&mut self, now: Instant) {
        let open: Vec<_> = self
            .panel_items()
            .filter(|panel| panel.morph.target() > 0.0)
            .map(|panel| (self.panel_frame(panel, now).open, panel.from.clone()))
            .collect();
        for pill in self.rows.iter_mut().flat_map(|row| row.iter_mut()) {
            let under = pill.bounds.get().is_some_and(|pill_box| {
                open.iter().any(|(panel, from)| from.as_ref() != Some(&pill.view) && overlaps(*panel, pill_box))
            });
            pill.shown.set_target(if under { 0.0 } else { 1.0 }, now);
        }
    }

    pub(crate) fn moving(&self, now: Instant) -> bool {
        self.panel_items().any(|panel| panel.moving(now))
            || self.rows.iter().flat_map(|row| row.iter()).any(|pill| !pill.shown.settled(now))
    }

    /// How far the panel growing out of `pill` has come: the furthest of
    /// them, when several grow out of one pill.
    fn grown_out_of(&self, pill: &PillItem, now: Instant) -> f32 {
        self.panel_items()
            .filter(|panel| panel.from.as_ref() == Some(&pill.view))
            .map(|panel| panel.morph.value(now))
            .fold(0.0, f32::max)
    }

    /// An element that records the glass it fills as one of this frame's
    /// shapes, and remembers the box in `keep` when given. A box that
    /// moved while a panel is drawn costs one more frame: the panel was
    /// laid out by the box of the frame before.
    fn record(&self, radius: Pixels, counts: bool, keep: Option<(PillBox, bool)>) -> impl IntoElement {
        let shapes = self.shapes.clone();
        canvas(
            move |bounds, window, _| {
                if let Some((keep, panel_drawn)) = keep
                    && keep.replace(Some(bounds)) != Some(bounds)
                    && panel_drawn
                {
                    window.request_animation_frame();
                }
                if counts {
                    shapes.borrow_mut().push(Shape::new(bounds, radius));
                }
            },
            |_, (), _, _| {},
        )
        .absolute()
        .inset_0()
    }

    fn panel(&self, panel: &PanelItem, now: Instant, cx: &App) -> Option<AnyElement> {
        if panel.closed(now) {
            return None;
        }
        let frame = self.panel_frame(panel, now);
        let content_opacity = panel.reveal.opacity(panel.morph.value(now), now);
        let content = div()
            .absolute()
            .left(frame.content_at.x)
            .top(frame.content_at.y)
            .w(frame.open.size.width)
            .h(frame.open.size.height)
            .opacity(content_opacity)
            .child(panel.spec.content.clone());
        let glass = Glass::new(frame.radius)
            .shadow(frame.opacity)
            .build(cx)
            .absolute()
            .left(frame.glass.left())
            .top(frame.glass.top())
            .w(frame.glass.size.width)
            .h(frame.glass.size.height)
            .opacity(frame.opacity)
            .child(self.record(frame.radius, true, None))
            // The glass can't clip (its shadow lies outside it), so a child does.
            .child(div().absolute().inset_0().overflow_hidden().child(content));
        Some(glass.into_any_element())
    }

    fn pill(&self, pill: &PillItem, now: Instant, cx: &mut Context<Self>) -> AnyElement {
        let grown = self.grown_out_of(pill, now);
        let panel_drawn = self.panel_items().any(|panel| !panel.closed(now));
        // Its panel's glass has taken its place.
        let replaced = grown > 0.0;
        let opacity = pill.shown.value(now) * (1.0 - grown / PILL_FADE_OUT).clamp(0.0, 1.0);
        let shown = opacity > 0.0;
        let radius = BAR_HEIGHT / 2.0;
        let slot = if replaced { div() } else { Glass::new(radius).shadow(opacity).build(cx) };
        let content = div().h_full().flex().items_center().child(pill.content.clone());
        let slot = slot
            .h(BAR_HEIGHT)
            .flex_none()
            .opacity(opacity)
            .child(self.record(radius, shown && !replaced, Some((pill.bounds.clone(), panel_drawn))))
            .child(if shown { content } else { content.invisible() });
        let Some(name) = pill.panel.as_ref().map(|panel| panel.spec.name.clone()) else {
            return slot.into_any_element();
        };
        if !(shown && pill.opens_on_click) {
            return slot.into_any_element();
        }
        slot.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |stage, _: &MouseDownEvent, _, cx| Stage::toggle(&name, stage.screen.id, cx)),
        )
        .into_any_element()
    }

    /// The bar: the left row from the left margin, the right row up to the
    /// right margin, the center row in the middle of the screen.
    fn rows(&self, now: Instant, cx: &mut Context<Self>) -> Div {
        let mut rows = ROWS.iter().zip(&self.rows).map(|((_, align), row)| {
            let pills: Vec<AnyElement> = row.iter().map(|pill| self.pill(pill, now, cx)).collect();
            let row = div().flex().items_center().gap(GAP).children(pills);
            match align {
                Align::Left => row.flex_1().justify_start(),
                Align::Center => row.flex_none(),
                Align::Right => row.flex_1().justify_end(),
            }
        });
        let bar = div().absolute().top(BAR_GAP).left(BAR_MARGIN).right(BAR_MARGIN).h(BAR_HEIGHT).flex().items_center();
        bar.children(rows.by_ref())
    }

    /// Hands this frame's shapes to the compositor, when they changed: it
    /// blurs behind them, and clicks land on them alone, or anywhere on the
    /// stage while a panel that a click outside closes is open.
    fn regions(&self) -> impl IntoElement {
        let (shapes, sent, catching) = (self.shapes.clone(), self.sent.clone(), self.catching);
        canvas(
            |_, _, _| {},
            move |_, (), window, _| {
                let drawn = (shapes.take(), catching);
                if sent.borrow().as_ref() != Some(&drawn) {
                    set_blur_region(window, &drawn.0);
                    let clicks = region(&drawn.0);
                    window.set_input_region(if catching { None } else { Some(&clicks) });
                    sent.replace(Some(drawn.clone()));
                }
                if catching {
                    dismiss_outside(drawn.0, window);
                }
            },
        )
        .absolute()
        .size_0()
    }
}

/// Closes the panels on a click or a scroll off `shapes`. A scroll closes
/// them too, so the next one reaches the window below. The handlers see
/// every click the window gets, also one outside it: Hyprland sends a
/// surface that holds the keyboard the clicks of every screen.
fn dismiss_outside(shapes: Vec<Shape>, window: &mut Window) {
    let shapes = Rc::new(shapes);
    let on_scroll = shapes.clone();
    window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble && !covered(&shapes, event.position) {
            Stage::dismiss(cx);
        }
    });
    window.on_mouse_event(move |event: &ScrollEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble && !covered(&on_scroll, event.position) {
            Stage::dismiss(cx);
        }
    });
}

impl Render for StageView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        self.give_way(now);
        self.lower_once_closed(window, cx, now);
        let moving = self.moving(now);
        if moving {
            window.request_animation_frame();
        }
        if let Some(frames) = &mut self.frames {
            for line in frames.frame(moving, now) {
                eprintln!("INFO nova-ui: {line}");
            }
        }
        self.shapes.borrow_mut().clear();

        let panels: Vec<AnyElement> = self.panel_items().filter_map(|panel| self.panel(panel, now, cx)).collect();
        div()
            .size_full()
            .body_text(cx)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == ESCAPE {
                    Stage::dismiss(cx);
                }
            }))
            .children(panels)
            .child(self.rows(now, cx))
            .child(self.regions())
    }
}
