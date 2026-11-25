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

pub(crate) const ESCAPE: &str = "escape";

pub(crate) type Shapes = Rc<RefCell<Vec<Shape>>>;

pub(crate) struct PanelFrame {
    pub glass: Bounds<Pixels>,
    pub radius: Pixels,
    pub open: Bounds<Pixels>,
    pub opacity: f32,
    pub content_opacity: f32,
    pub glassy: bool,
    pub fillet: Pixels,
}

impl PanelFrame {
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
    pub(crate) fn panel_frame(&self, panel: &PanelItem, now: Instant) -> PanelFrame {
        let from = panel.from.as_ref().and_then(|view| self.pill_box(view));
        frame(panel, from, self.screen.size, now)
    }

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

    pub(crate) fn moving(&self, now: Instant) -> bool {
        let moves_here = |panel: &PanelItem| {
            panel.on_strip() || panel.from.as_ref().is_some_and(|pill| self.pill_box(pill).is_some())
        };
        self.aside.moving(now)
            || self.panel_items().any(|panel| panel.moving(now) && moves_here(panel))
            || self.rows.iter().flat_map(|row| row.iter()).any(|pill| !pill.shown.settled(now))
    }

    fn grown_out_of(&self, pill: &PillItem, now: Instant) -> f32 {
        self.panel_items()
            .filter(|panel| panel.from.as_ref() == Some(&pill.view) && (panel.on_strip() || panel.cover.get()))
            .map(|panel| panel.morph.value(now))
            .fold(0.0, f32::max)
    }
}

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

pub(crate) fn panel(
    panel: &PanelItem,
    frame: &PanelFrame,
    origin: Point<Pixels>,
    shapes: &Shapes,
    cx: &App,
) -> AnyElement {
    let content = div()
        .absolute()
        .left(frame.open.left() - frame.glass.left())
        .top(frame.open.top() - frame.glass.top())
        .w(frame.open.size.width)
        .h(frame.open.size.height)
        .opacity(frame.content_opacity)
        .child(panel.spec.content.clone());
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
        // The compositor's blur can't fade: a fading panel keeps no shape, or an empty frosted box stays and takes clicks:
        // https://linear.app/imperaluna/issue/IMP-57
        .child(record(shapes, frame.radius, !fades_out(panel, frame), None))
        .child(div().absolute().inset_0().overflow_hidden().child(content))
        .children(fillets(frame, origin, shapes))
        .into_any_element()
}

fn fades_out(panel: &PanelItem, frame: &PanelFrame) -> bool {
    frame.opacity < 1.0 && panel.morph.target() == 0.0
}

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
    fn pill_opacity(&self, pill: &PillItem, now: Instant) -> f32 {
        let grown = self.grown_out_of(pill, now);
        pill.shown.value(now) * (1.0 - grown / PILL_FADE_OUT).clamp(0.0, 1.0)
    }

    fn pill(&self, pill: &PillItem, now: Instant, cx: &mut Context<Self>) -> AnyElement {
        let grown = self.grown_out_of(pill, now);
        let panel_drawn = self.panel_items().any(|panel| !panel.closed(now));
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
                (Align::Center, Some(from_right)) => {
                    row.absolute().top_0().bottom_0().right(screen_middle - from_right - BAR_MARGIN)
                }
                (Align::Right, _) => row.flex_1().justify_end(),
            }
        });
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

    fn regions(&self, now: Instant) -> impl IntoElement + use<> {
        let (shapes, sent, catching) = (self.shapes.clone(), self.sent.clone(), self.catching);
        let (beside, hand_back) = (self.beside.get(), self.hand_back(now));
        let placed_by_middle = self.middle_from_right.is_some() && self.center_drawn;
        let strip = self.aside.fullscreen();
        let screen = self.screen.size;
        canvas(
            |_, _, _| {},
            move |_, (), window, cx| {
                if placed_by_middle && !middle::recorded(window, cx) {
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

pub(crate) fn dismiss_outside(shapes: Vec<Shape>, window: &mut Window) {
    let shapes = Rc::new(shapes);
    let on_scroll = shapes.clone();
    // Hyprland sends a surface that holds the keyboard the clicks of every screen, outside the window too:
    // https://linear.app/imperaluna/issue/IMP-396
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
        self.ask_layer_once_on_screen(window, cx);
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
