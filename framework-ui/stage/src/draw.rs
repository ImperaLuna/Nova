//! Drawing a screen's stage. Back to front: the panels that are open or on
//! their way, then the bar's three rows of pills, then (drawing nothing)
//! the regions. A panel placed off the strip is laid out here
//! ([`StageView::panel_frame`], in screen coordinates, which are the
//! strip's own) and drawn by the window beside (`crate::panel_window`)
//! with the same [`panel`].
//!
//! A panel's glass is the box between its pill and its open place, so the
//! pill seems to grow: the pill's own glass steps aside the moment its
//! panel starts, its content fades out, and the panel's content, laid out
//! at its open size and clipped to the glass, fades in once there is room.
//! A pill an open panel lies over fades away, since glass is see-through.
//! While the bar steps aside for a fullscreen window (`crate::aside`) its
//! rows slide up out of sight and fade, and only a strip along the top edge
//! takes the pointer, which brings them back.
//!
//! No feature tells the compositor where anything is. Every glass records
//! its box while the frame is laid out, and the frame's last element hands
//! the list to the compositor as the window's blur and click regions, only
//! when it differs from the one sent before. Frames are asked for only
//! while something here is on its way; a view that changes by itself (a
//! clock) redraws through GPUI like any view.

use crate::geometry::{
    Align, between, covered, edge_box, edge_origin, left_of_panels, open_box, overlaps, past_edge, radius_between,
    tab_box, tab_nub,
};
use crate::item::Placement;
use crate::middle;
use crate::stage::Stage;
use crate::tokens::{PANEL_ENTER_RISE, PILL_FADE_OUT};
use crate::view::{CENTER_ROW, PanelItem, PillBox, PillItem, ROWS, StageView};
use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, Div, HoverListenerMode, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, Pixels, Point, Render, ScrollEvent, Size,
    StatefulInteractiveElement, Styled, Window, canvas, div, point, px, size,
};
use nova_look::tokens::{
    BAR_GAP, BAR_HEIGHT, BAR_MARGIN, EDGE_TAB_FILLET, EDGE_TAB_NUB, GAP, REVEAL_STRIP, SHADOW_ROOM,
};
use nova_look::{Fillet, FilletSide, Glass, LookStyled, Shape, region, set_blur_region};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// The key that closes panels, as GPUI names it.
pub(crate) const ESCAPE: &str = "escape";

/// The shapes a window drew in the frame under way.
pub(crate) type Shapes = Rc<RefCell<Vec<Shape>>>;

/// Where a panel is drawn right now, on its screen.
pub(crate) struct PanelFrame {
    /// Its box: the glass, or all of a panel that has none.
    pub glass: Bounds<Pixels>,
    pub radius: Pixels,
    /// Where its content is laid out: its place once open.
    pub open: Bounds<Pixels>,
    /// The whole panel's opacity: below 1 only for one that fades in or
    /// out, content and all.
    pub opacity: f32,
    /// Its content's opacity inside that.
    pub content_opacity: f32,
    /// Whether it is glass; a panel over the whole screen draws all it is.
    pub glassy: bool,
    /// How big the concave corners are that curve a tab into the right
    /// edge above and below it; nothing for the other placements.
    pub fillet: Pixels,
}

impl PanelFrame {
    /// A panel `progress` of the way out of `pill`: its content is laid
    /// out at `open` and its glass heads for `target`, which is `open`
    /// unless the glass ends up bigger than what it holds.
    fn grown(
        pill: Bounds<Pixels>,
        open: Bounds<Pixels>,
        target: Bounds<Pixels>,
        panel: &PanelItem,
        now: Instant,
    ) -> Self {
        let progress = panel.morph.value(now);
        Self {
            glass: between(pill, target, progress),
            radius: radius_between(panel.spec.radius, progress),
            open,
            opacity: 1.0,
            content_opacity: panel.reveal.opacity(progress, now),
            glassy: true,
            fillet: px(0.0),
        }
    }
}

/// Where `panel` is at `now`, on a screen `screen` big, growing out of
/// the pill at `from` when it has one that is there.
pub(crate) fn frame(
    panel: &PanelItem,
    from: Option<(Bounds<Pixels>, Align)>,
    screen: Size<Pixels>,
    now: Instant,
) -> PanelFrame {
    let progress = panel.morph.value(now);
    let open_size = size(px(panel.width.value(now)), px(panel.height.value(now)));
    match (panel.spec.placement, from) {
        (Placement::Top, Some((pill, _))) => {
            let open = open_box(from, None, open_size, panel.spec.top, screen.width);
            PanelFrame::grown(pill, open, open, panel, now)
        }
        (Placement::Top, None) => {
            let open = open_box(None, panel.left, open_size, panel.spec.top, screen.width);
            let risen = PANEL_ENTER_RISE * (1.0 - progress);
            PanelFrame {
                glass: Bounds { origin: point(open.left(), open.top() - risen), size: open.size },
                radius: panel.spec.radius,
                open: Bounds { origin: point(open.left(), open.top() - risen), size: open.size },
                opacity: progress,
                // Its content fades with its glass: waiting for room to
                // grow would leave the glass empty, and there is none.
                content_opacity: 1.0,
                glassy: true,
                fillet: px(0.0),
            }
        }
        (Placement::RightEdge, _) => {
            let open = edge_box(open_size.width, screen);
            let pill = from.map_or_else(|| edge_origin(screen.width), |(pill, _)| pill);
            PanelFrame::grown(pill, open, past_edge(open, panel.spec.radius), panel, now)
        }
        (Placement::WholeScreen, _) => {
            let all = Bounds { origin: point(px(0.0), px(0.0)), size: screen };
            PanelFrame {
                glass: all,
                radius: px(0.0),
                open: all,
                opacity: progress,
                content_opacity: 1.0,
                glassy: false,
                fillet: px(0.0),
            }
        }
        (Placement::RightTab, _) => tab_frame(panel, open_size, screen, now),
    }
}

/// Where a tab against the right edge is at `now`, `open_size` big once open:
/// it grows out of a nub at the edge, its corners never rounder than half its
/// height, and its fillets grow with it.
fn tab_frame(panel: &PanelItem, open_size: Size<Pixels>, screen: Size<Pixels>, now: Instant) -> PanelFrame {
    let progress = panel.morph.value(now);
    let open = tab_box(open_size, screen);
    let radius = panel.spec.radius;
    let glass = between(tab_nub(EDGE_TAB_NUB, radius, screen), past_edge(open, radius), progress);
    PanelFrame {
        glass,
        radius: radius.min(glass.size.height / 2.0),
        open,
        opacity: 1.0,
        content_opacity: panel.reveal.opacity(progress, now),
        glassy: true,
        fillet: EDGE_TAB_FILLET * progress.clamp(0.0, 1.0),
    }
}

impl StageView {
    /// Where `panel` is at `now`.
    pub(crate) fn panel_frame(&self, panel: &PanelItem, now: Instant) -> PanelFrame {
        let from = panel.from.as_ref().and_then(|view| self.pill_box(view));
        frame(panel, from, self.screen.size, now)
    }

    /// Hides each pill an open panel lies over, and shows the others. A
    /// panel over the whole screen (the wallpaper carousel) lies under the
    /// strip, which opened after its window, and leaves the bar drawn over
    /// it.
    fn give_way(&mut self, now: Instant) {
        let open: Vec<_> = self
            .panel_items()
            .filter(|panel| {
                panel.spec.placement != Placement::WholeScreen && !panel.spec.in_place && panel.morph.target() > 0.0
            })
            .map(|panel| (self.panel_frame(panel, now).open, panel.from.clone()))
            .collect();
        for pill in self.rows.iter_mut().flat_map(|row| row.iter_mut()) {
            let under = pill.bounds.get().is_some_and(|pill_box| {
                open.iter().any(|(panel, from)| from.as_ref() != Some(&pill.view) && overlaps(*panel, pill_box))
            });
            pill.shown.set_target(if under { 0.0 } else { 1.0 }, now);
        }
    }

    /// Whether this window has frames to draw. A panel in the window
    /// beside moves there; here only the pill it grows out of changes.
    pub(crate) fn moving(&self, now: Instant) -> bool {
        let moves_here = |panel: &PanelItem| {
            panel.on_strip() || panel.from.as_ref().is_some_and(|pill| self.pill_box(pill).is_some())
        };
        self.aside.moving(now)
            || self.panel_items().any(|panel| panel.moving(now) && moves_here(panel))
            || self.rows.iter().flat_map(|row| row.iter()).any(|pill| !pill.shown.settled(now))
    }

    /// How far the panel growing out of `pill` has come: the furthest of
    /// them, when several grow out of one pill. One in the window beside
    /// counts only once it is on screen there (`PanelItem::cover`).
    fn grown_out_of(&self, pill: &PillItem, now: Instant) -> f32 {
        self.panel_items()
            .filter(|panel| panel.from.as_ref() == Some(&pill.view) && (panel.on_strip() || panel.cover.get()))
            .map(|panel| panel.morph.value(now))
            .fold(0.0, f32::max)
    }
}

/// An element that records the box it fills as one of this frame's
/// `shapes`, and remembers the box in `keep` when given. A box that
/// moved while a panel is drawn costs one more frame: the panel was
/// laid out by the box of the frame before.
fn record(shapes: &Shapes, radius: Pixels, counts: bool, keep: Option<(PillBox, bool)>) -> impl IntoElement + use<> {
    let shapes = shapes.clone();
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

/// Draws `panel` at `frame`, in a window whose top left corner is at
/// `origin` on the screen, and records its box in `shapes`. A panel that
/// fades out records none: the compositor's blur can't fade, so it would
/// stay an empty frosted box while the glass fades over it, and a click
/// there reaches the window below.
pub(crate) fn panel(
    panel: &PanelItem,
    frame: &PanelFrame,
    origin: Point<Pixels>,
    shapes: &Shapes,
    cx: &App,
) -> AnyElement {
    let content = div()
        .absolute()
        // The content stays where it will be; the glass grows over it.
        .left(frame.open.left() - frame.glass.left())
        .top(frame.open.top() - frame.glass.top())
        .w(frame.open.size.width)
        .h(frame.open.size.height)
        .opacity(frame.content_opacity)
        .child(panel.spec.content.clone());
    // A tab's edge runs on into its fillets, from where they start.
    let rim_cut =
        if frame.fillet > px(0.0) { frame.glass.right() - frame.open.right() + frame.fillet } else { px(0.0) };
    let glass = Glass::new(frame.radius).shadow(frame.opacity).rim_cut(rim_cut);
    let back = if frame.glassy { glass.build(cx) } else { div() };
    back.absolute()
        .left(frame.glass.left() - origin.x)
        .top(frame.glass.top() - origin.y)
        .w(frame.glass.size.width)
        .h(frame.glass.size.height)
        .opacity(frame.opacity)
        .child(record(shapes, frame.radius, !fades_out(panel, frame), None))
        // The glass can't clip (its shadow lies outside it), so a child does.
        .child(div().absolute().inset_0().overflow_hidden().child(content))
        .children(fillets(frame, origin, shapes))
        .into_any_element()
}

/// Whether `panel` is fading out at `frame`: it heads for closed and lets
/// its whole look go, rather than shrinking back into a pill.
fn fades_out(panel: &PanelItem, frame: &PanelFrame) -> bool {
    frame.opacity < 1.0 && panel.morph.target() == 0.0
}

/// The concave corners that curve a tab into the right edge (`frame.fillet`
/// big, none for other panels), placed on its glass, with their shapes
/// recorded for the regions. The edge is where the tab's content ends.
fn fillets(frame: &PanelFrame, origin: Point<Pixels>, shapes: &Shapes) -> Vec<AnyElement> {
    let fillet = frame.fillet;
    if fillet <= px(0.0) {
        return Vec::new();
    }
    let left = frame.open.right() - fillet;
    [(FilletSide::Above, frame.glass.top() - fillet), (FilletSide::Below, frame.glass.bottom())]
        .into_iter()
        .map(|(side, top)| {
            let on_window = Bounds { origin: point(left - origin.x, top - origin.y), size: size(fillet, fillet) };
            shapes.borrow_mut().extend(Fillet::shapes(side, on_window));
            div()
                .absolute()
                .left(left - frame.glass.left())
                .top(top - frame.glass.top())
                .size(fillet)
                .child(Fillet::new(side))
                .into_any_element()
        })
        .collect()
}

impl StageView {
    /// How much of `pill` shows: none under another pill's open panel, or
    /// once its own panel has grown far enough. At none its content is
    /// laid out but not drawn.
    fn pill_opacity(&self, pill: &PillItem, now: Instant) -> f32 {
        let grown = self.grown_out_of(pill, now);
        pill.shown.value(now) * (1.0 - grown / PILL_FADE_OUT).clamp(0.0, 1.0)
    }

    fn pill(&self, pill: &PillItem, now: Instant, cx: &mut Context<Self>) -> AnyElement {
        let grown = self.grown_out_of(pill, now);
        let panel_drawn = self.panel_items().any(|panel| !panel.closed(now));
        // Its panel's glass has taken its place.
        let replaced = grown > 0.0;
        let opacity = self.pill_opacity(pill, now);
        let shown = opacity > 0.0;
        let radius = BAR_HEIGHT / 2.0;
        let slot = if replaced { div() } else { Glass::new(radius).shadow(opacity).build(cx) };
        let content = div().h_full().flex().items_center().child(pill.content.clone());
        let slot = slot
            .h(BAR_HEIGHT)
            .flex_none()
            .opacity(opacity)
            .child(record(&self.shapes, radius, shown && !replaced, Some((pill.bounds.clone(), panel_drawn))))
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
    /// right margin, the center row in the middle of the screen, or with
    /// the point a view marks (`middle.rs`) there. A pill with nothing to
    /// show takes no room. Nothing while the bar is out of sight for a
    /// fullscreen window, so its pills take no clicks and need no blur.
    fn rows(&self, now: Instant, cx: &mut Context<Self>) -> Option<Div> {
        if self.aside.gone(now) {
            return None;
        }
        let middle_from_right = self.middle_from_right;
        let screen_middle = self.screen.size.width / 2.0;
        let mut rows = ROWS.iter().zip(&self.rows).map(|((_, align), row)| {
            let pills: Vec<AnyElement> = row
                .iter()
                .filter(|pill| {
                    let present = pill.is_present();
                    if !present {
                        pill.bounds.set(None);
                    }
                    present
                })
                .map(|pill| self.pill(pill, now, cx))
                .collect();
            let row = div().flex().items_center().gap(GAP).children(pills);
            match (align, middle_from_right) {
                (Align::Left, _) => row.flex_1().justify_start(),
                (Align::Center, None) => row.flex_none(),
                // Its right edge that far right of the middle, in the bar.
                (Align::Center, Some(from_right)) => {
                    row.absolute().top_0().bottom_0().right(screen_middle - from_right - BAR_MARGIN)
                }
                (Align::Right, _) => row.flex_1().justify_end(),
            }
        });
        // Slid up with its shadow, so nothing of it shows at the top edge.
        let slid = self.aside.slid(now);
        let above = (BAR_GAP + BAR_HEIGHT + SHADOW_ROOM) * slid;
        let bar = div()
            .absolute()
            .top(BAR_GAP - above)
            .left(BAR_MARGIN)
            .right(BAR_MARGIN)
            .h(BAR_HEIGHT)
            .flex()
            .items_center()
            .opacity(1.0 - slid);
        Some(bar.children(rows.by_ref()))
    }

    /// Reads the point the last frame marked for the screen's middle, and
    /// its distance from the center row's right edge then.
    ///
    /// While the center row's content isn't drawn (a panel grew out of it
    /// or lies over it, or the bar stepped aside), nothing marks the middle: the row keeps its place,
    /// so a panel closing into its pill lands where the pill comes back.
    fn follow_middle(&mut self, window: &Window, cx: &mut App, now: Instant) {
        let marked = middle::read(window, cx);
        let drawn_before = self.center_drawn;
        self.center_drawn = !self.aside.gone(now)
            && self.rows[CENTER_ROW]
                .iter()
                .filter(|pill| pill.is_present())
                .all(|pill| self.pill_opacity(pill, now) > 0.0);
        if marked.is_none() && !drawn_before {
            return;
        }
        let row_right = self.rows[CENTER_ROW]
            .iter()
            .filter(|pill| pill.is_present())
            .filter_map(|pill| pill.bounds.get())
            .map(|pill| pill.right())
            .reduce(Pixels::max);
        self.middle_from_right = match (marked, row_right) {
            (Some(at), Some(right)) => Some(right - at),
            _ => None,
        };
    }

    /// The panels of the window beside that have closed while it still
    /// shows them: this frame draws their pills again, so once it is
    /// painted that window lets go of them. Returns what does that.
    fn hand_back(&self, now: Instant) -> impl FnOnce(&mut App) + use<> {
        let closed = |panel: &&PanelItem| !panel.on_strip() && panel.closed(now) && panel.cover.get();
        let covers: Vec<_> = self.panel_items().filter(closed).map(|panel| panel.cover.clone()).collect();
        let beside = self.panel_window;
        move |cx| {
            for cover in &covers {
                cover.set(false);
            }
            if let Some(beside) = beside.filter(|_| !covers.is_empty()) {
                cx.defer(move |cx| {
                    let _ = beside.update(cx, |_, _, cx| cx.notify());
                });
            }
        }
    }

    /// Hands this frame's shapes to the compositor, when they changed: it
    /// blurs behind them, and clicks land on them alone, or anywhere on the
    /// stage (but where the panels' window beside is) while a panel that a
    /// click outside closes is open.
    fn regions(&self, now: Instant) -> impl IntoElement + use<> {
        let (shapes, sent, catching) = (self.shapes.clone(), self.sent.clone(), self.catching);
        let (beside, hand_back) = (self.beside.get(), self.hand_back(now));
        // A row placed by a mark that is drawn this frame expects it again.
        let placed_by_middle = self.middle_from_right.is_some() && self.center_drawn;
        let strip = self.aside.fullscreen();
        let screen = self.screen.size;
        canvas(
            |_, _, _| {},
            move |_, (), window, cx| {
                if placed_by_middle && !middle::recorded(window, cx) {
                    // What marked the middle is gone: center the row again.
                    window.request_animation_frame();
                }
                hand_back(cx);
                let drawn = (shapes.take(), catching, strip, beside);
                if sent.borrow().as_ref() != Some(&drawn) {
                    set_blur_region(window, &drawn.0);
                    let mut clicks = region(&drawn.0);
                    if strip {
                        clicks.push(Bounds { origin: point(px(0.0), px(0.0)), size: size(screen.width, REVEAL_STRIP) });
                    }
                    if catching {
                        // As far down as the screen: the window ends sooner.
                        clicks = vec![left_of_panels(screen.height, beside, screen.width)];
                    }
                    let everywhere = catching && beside == px(0.0);
                    window.set_input_region(if everywhere { None } else { Some(&clicks) });
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
pub(crate) fn dismiss_outside(shapes: Vec<Shape>, window: &mut Window) {
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

impl StageView {
    /// The pointer came onto the stage or left it: over a fullscreen window
    /// that brings the pills back, or lets them go again.
    fn hover(&mut self, hovered: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.aside.hovered() == hovered {
            return;
        }
        self.aside.set_hovered(hovered, Instant::now());
        if self.aside.fullscreen() {
            self.follow_aside(window, cx);
            cx.notify();
        }
    }
}

impl Render for StageView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        self.follow_middle(window, cx, now);
        self.give_way(now);
        self.rest_once_closed(window, cx, now);
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

        let origin = point(px(0.0), px(0.0));
        let on_strip = self.panel_items().filter(|panel| panel.on_strip() && !panel.closed(now));
        let panels: Vec<AnyElement> =
            on_strip.map(|each| panel(each, &self.panel_frame(each, now), origin, &self.shapes, cx)).collect();
        div()
            .id("stage")
            .size_full()
            .body_text(cx)
            .track_focus(&self.focus)
            // On the strip along the top edge or a pill: the window takes
            // the pointer nowhere else. A key pressed meanwhile leaves the
            // pointer where it is.
            .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
            .on_hover(cx.listener(|stage, hovered: &bool, window, cx| stage.hover(*hovered, window, cx)))
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == ESCAPE {
                    Stage::dismiss(cx);
                }
            }))
            .children(panels)
            .children(self.rows(now, cx))
            .child(self.regions(now))
    }
}
