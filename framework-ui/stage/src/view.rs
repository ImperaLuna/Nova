use crate::aside::Aside;
use crate::frames::FrameLog;
use crate::geometry::{Align, stage_height};
use crate::item::{Panel, Pill, Placement, Shown};
use crate::kept::Kept;
use crate::panel_window::{self, Hold, PanelWindow, Told};
use crate::registry;
use crate::reveal::Reveal;
use crate::satellites;
use crate::screens::{Screen, Screens};
use crate::slots::{HOST_BAR_CENTER, HOST_BAR_LEFT, HOST_BAR_RIGHT, HOST_STAGE_PANELS, Slots};
use crate::stage::Panels;
use crate::tokens::STAGE_NAMESPACE;
use gpui::{
    AnyView, AnyWindowHandle, App, AppContext, Bounds, Context, Entity, FocusHandle, Pixels, SharedString, Size,
    Subscription, Task, WeakEntity, Window, WindowHandle,
    layer_shell::{KeyboardInteractivity, Layer},
    px, size,
};
use nova_look::motion::Eased;
use nova_look::tokens::{DURATION_FADE, DURATION_FAST, DURATION_PANEL, DURATION_RESIZE};
use nova_look::{Shape, ShellWindow};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

pub(crate) const ROWS: [(&str, Align); 3] =
    [(HOST_BAR_LEFT, Align::Left), (HOST_BAR_CENTER, Align::Center), (HOST_BAR_RIGHT, Align::Right)];

pub(crate) const CENTER_ROW: usize = 1;

pub(crate) type PillBox = Rc<Cell<Option<Bounds<Pixels>>>>;

pub(crate) struct PillItem {
    pub view: SharedString,
    pub content: AnyView,
    pub opens_on_click: bool,
    pub present: Option<Shown>,
    pub seen_present: bool,
    pub bounds: PillBox,
    pub shown: Eased,
    pub panel: Option<PanelItem>,
}

impl PillItem {
    pub fn is_present(&self) -> bool {
        self.present.as_ref().is_none_or(Shown::get)
    }
}

#[derive(Clone)]
pub(crate) struct PanelItem {
    pub spec: Panel,
    pub from: Option<SharedString>,
    pub morph: Eased,
    pub reveal: Reveal,
    pub width: Eased,
    pub height: Eased,
    pub left: Option<Pixels>,
    pub cover: Rc<Cell<bool>>,
}

impl PanelItem {
    fn new(spec: Panel, own_pill: Option<&SharedString>, now: Instant) -> Self {
        let resizing = |length: Pixels| Eased::lasting(f32::from(length), DURATION_RESIZE, now);
        let grows = matches!(spec.placement, Placement::Top | Placement::RightEdge);
        Self {
            from: spec.from.clone().or_else(|| own_pill.cloned()).filter(|_| grows),
            morph: Eased::at(0.0, now),
            reveal: Reveal::hidden(now),
            width: resizing(spec.size.width),
            height: resizing(spec.size.height),
            left: None,
            cover: Rc::default(),
            spec,
        }
    }

    pub fn on_strip(&self) -> bool {
        self.spec.placement.on_strip()
    }

    fn set_open(&mut self, open: bool, now: Instant) -> bool {
        let target = if open { 1.0 } else { 0.0 };
        let turned = self.morph.target() != target;
        self.reveal.set_open(open, self.morph.value(now), now);
        self.morph.set_target(target, now);
        turned
    }

    fn set_size(&mut self, size: Size<Pixels>, now: Instant) -> bool {
        let (width, height) = (f32::from(size.width), f32::from(size.height));
        let changed = self.width.target() != width || self.height.target() != height;
        self.width.set_target(width, now);
        self.height.set_target(height, now);
        changed
    }

    pub fn closed(&self, now: Instant) -> bool {
        self.spec.in_place || (self.morph.target() == 0.0 && self.morph.settled(now))
    }

    pub fn moving(&self, now: Instant) -> bool {
        !self.spec.in_place
            && !(self.morph.settled(now)
                && self.reveal.settled(now)
                && self.width.settled(now)
                && self.height.settled(now))
    }

    fn reach(&self, now: Instant) -> Pixels {
        self.spec.top + px(self.height.target().max(self.height.value(now)))
    }
}

pub(crate) struct OffStrip {
    pub panel: PanelItem,
    pub pill: Option<(PillBox, Option<Shown>, Align)>,
}

impl OffStrip {
    pub fn pill_box(&self) -> Option<(Bounds<Pixels>, Align)> {
        let (bounds, present, align) = self.pill.as_ref()?;
        let there = present.as_ref().is_none_or(Shown::get);
        bounds.get().filter(|_| there).map(|bounds| (bounds, *align))
    }
}

pub(crate) type OffStripPanels = Rc<RefCell<Vec<OffStrip>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Held {
    keyboard: bool,
    overlay: bool,
}

pub(crate) struct StageView {
    pub screen: Screen,
    pub panels: Entity<Panels>,
    slots: Entity<Slots>,
    pub rows: [Kept<PillItem>; 3],
    pub standalone: Kept<PanelItem>,
    pub shapes: Rc<RefCell<Vec<Shape>>>,
    pub sent: Rc<RefCell<Sent>>,
    pub catching: bool,
    pub focus: FocusHandle,
    pub frames: Option<FrameLog>,
    pub middle_from_right: Option<Pixels>,
    pub center_drawn: bool,
    height: Rc<Cell<Pixels>>,
    held: Held,
    layer_asked_on_screen: bool,
    pub panel_window: Option<WindowHandle<PanelWindow>>,
    panel_window_due: bool,
    beside_held: Hold,
    pub beside: Rc<Cell<Pixels>>,
    off_strip: OffStripPanels,
    catcher: Option<AnyWindowHandle>,
    bar_room: Option<AnyWindowHandle>,
    pub aside: Aside,
    aside_wake: Option<Task<()>>,
    room_kept: bool,
    awake: bool,
    _observers: Vec<Subscription>,
}

pub(crate) type Sent = Option<(Vec<Shape>, bool, bool, Pixels)>;

pub(crate) struct Shared {
    pub panels: Entity<Panels>,
    pub slots: Entity<Slots>,
    pub screens: Entity<Screens>,
    pub log_frames: bool,
}

pub(crate) fn open(screen: &Screen, shared: &Shared, cx: &mut App) -> Option<WindowHandle<StageView>> {
    let Shared { panels, slots, screens, log_frames } = shared;
    let log_frames = *log_frames;
    let height = stage_height([]);
    let mut shell = ShellWindow::new(screen.id, size(screen.size.width, height));
    shell.namespace = STAGE_NAMESPACE;
    // niri stacks the surfaces of one layer by when they opened, so the panels' window opens first:
    // https://linear.app/imperaluna/issue/IMP-427
    let (off_strip, beside) = (OffStripPanels::default(), Rc::new(Cell::new(px(0.0))));
    let shared_beside =
        panel_window::Shared { stage: WeakEntity::new_invalid(), panels: off_strip.clone(), beside: beside.clone() };
    let panel_window = panel_window::open(screen, shared_beside, log_frames, cx);
    let (screen, panels, slots, screens) = (screen.clone(), panels.clone(), slots.clone(), screens.clone());
    let opened = cx.open_window(shell.options(), move |window, cx| {
        cx.new(|cx| {
            let now = Instant::now();
            let observers = vec![
                cx.observe_in(&slots, window, |stage: &mut StageView, _, window, cx| stage.sync_items(window, cx)),
                cx.observe_in(&panels, window, |stage: &mut StageView, _, window, cx| stage.follow_panels(window, cx)),
                cx.observe_in(&screens, window, StageView::follow_screen),
                Shown::observe(cx, StageView::follow_shown),
                cx.on_release(|stage: &mut StageView, cx| stage.leave(cx)),
            ];
            let frames = log_frames.then(|| FrameLog::new(screen.name.to_string(), screen.refresh, now));
            let mut stage = StageView {
                screen,
                panels,
                slots,
                rows: Default::default(),
                standalone: Kept::default(),
                shapes: Rc::default(),
                sent: Rc::default(),
                catching: false,
                focus: cx.focus_handle(),
                frames,
                middle_from_right: None,
                center_drawn: true,
                height: Rc::new(Cell::new(height)),
                held: Held::default(),
                layer_asked_on_screen: false,
                panel_window,
                panel_window_due: false,
                beside_held: Hold::default(),
                beside,
                off_strip,
                catcher: None,
                bar_room: None,
                aside: Aside::new(now),
                aside_wake: None,
                room_kept: true,
                awake: false,
                _observers: observers,
            };
            stage.sync_items(window, cx);
            if stage.panels.read(cx).awake {
                stage.awake = true;
                window.set_idle_inhibit(true);
            }
            stage
        })
    });
    match opened {
        Ok(window) => {
            if let (Some(beside), Ok(stage)) = (panel_window, window.update(cx, |_, _, cx| cx.weak_entity())) {
                let _ = beside.update(cx, |beside, _, _| beside.set_stage(stage));
                let _ = window.update(cx, |stage, _, cx| stage.panel_window_changed(cx));
            }
            Some(window)
        }
        Err(error) => {
            eprintln!("ERROR nova-ui: can't open the stage: {error:#}");
            if let Some(beside) = panel_window {
                satellites::close(beside.into(), cx);
            }
            None
        }
    }
}

impl StageView {
    pub fn panel_items(&self) -> impl Iterator<Item = &PanelItem> {
        let of_pills = self.rows.iter().flat_map(|row| row.iter()).filter_map(|pill| pill.panel.as_ref());
        of_pills.chain(self.standalone.iter())
    }

    fn panel_items_mut(&mut self) -> impl Iterator<Item = &mut PanelItem> {
        let of_pills = self.rows.iter_mut().flat_map(|row| row.iter_mut()).filter_map(|pill| pill.panel.as_mut());
        of_pills.chain(self.standalone.iter_mut())
    }

    pub fn pill_box(&self, view: &SharedString) -> Option<(Bounds<Pixels>, Align)> {
        self.rows.iter().zip(ROWS).find_map(|(row, (_, align))| {
            let pill = row.iter().find(|pill| pill.view == *view && pill.is_present())?;
            Some((pill.bounds.get()?, align))
        })
    }

    fn sync_items(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let screen = self.screen.clone();
        let slots = self.slots.read(cx);
        let wanted_rows = ROWS.map(|(host, _)| slots.for_host(host, cx));
        let wanted_panels = slots.for_host(HOST_STAGE_PANELS, cx);
        for ((row, (host, _)), wanted) in self.rows.iter_mut().zip(ROWS).zip(&wanted_rows) {
            row.sync(wanted, |view| {
                let Pill { content, panel, opens_on_click, shown } = registry::build_pill(view, host, &screen, cx)?;
                Some(PillItem {
                    view: view.clone(),
                    content,
                    opens_on_click,
                    seen_present: shown.as_ref().is_none_or(Shown::get),
                    present: shown,
                    bounds: Rc::default(),
                    shown: Eased::lasting(1.0, DURATION_FAST, now),
                    panel: panel.map(|panel| PanelItem::new(panel, Some(view), now)),
                })
            });
        }
        self.standalone.sync(&wanted_panels, |view| {
            registry::build_panel(view, HOST_STAGE_PANELS, &screen, cx).map(|panel| PanelItem::new(panel, None, now))
        });

        let rules = self.panel_items().map(|panel| panel.spec.rules()).collect();
        let id = self.screen.id;
        self.panels.update(cx, |panels, cx| {
            if panels.know(id, rules) {
                cx.notify();
            }
        });
        self.keep_panel_window(cx);
        self.follow_panels(window, cx);
        self.fit(window, cx, true);
        self.keep_bar_room(cx);
        cx.notify();
    }

    fn follow_shown(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        for pill in self.rows.iter_mut().flat_map(|row| row.iter_mut()) {
            let present = pill.is_present();
            changed |= present != pill.seen_present;
            pill.seen_present = present;
        }
        if changed {
            cx.notify();
        }
    }

    fn follow_panels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let id = self.screen.id;
        let panels = self.panels.read(cx);
        if panels.awake != self.awake {
            self.awake = panels.awake;
            window.set_idle_inhibit(self.awake);
        }
        let has_bar = self.rows.iter().any(|row| !row.is_empty());
        let fullscreen = has_bar && panels.fullscreen(&self.screen.name);
        let any_open = panels.open.any_open(id);
        // Hyprland gives a Top layer under a focused fullscreen window no pixels and no pointer:
        // https://linear.app/imperaluna/issue/IMP-182
        let overlay = any_open || self.held.overlay || fullscreen;
        let held = Held { keyboard: panels.open.needs_keyboard(id, true), overlay };
        let catching = panels.open.catches_clicks();
        let beside = Hold { keyboard: panels.open.needs_keyboard(id, false), catching };
        let targets: Vec<(bool, Option<Size<Pixels>>, Option<Pixels>)> = self
            .panel_items()
            .map(|panel| {
                let name = &panel.spec.name;
                (panels.open.is_open(name, id), panels.size(name, id), panels.place(name, id))
            })
            .collect();
        let (mut focus, mut beside_focus, mut beside_moved) = (None, None, false);
        for (panel, (open, size, left)) in self.panel_items_mut().zip(targets) {
            let opened = open && panel.morph.target() == 0.0;
            let moved = panel.set_open(open, now) | size.is_some_and(|size| panel.set_size(size, now));
            panel.left = left;
            beside_moved |= moved && !panel.on_strip();
            if opened && panel.spec.rules().closes_on_esc() {
                let field = Some(panel.spec.focus.clone());
                if panel.on_strip() { focus = field } else { beside_focus = field }
            }
        }
        self.publish_off_strip();
        self.tell_panel_window(beside, beside_focus, beside_moved, cx);

        if held.keyboard != self.held.keyboard {
            let keyboard = if held.keyboard { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None };
            window.set_keyboard_interactivity(keyboard);
        }
        // Hyprland 0.57 drops a layer change committed before a surface's first frame; once it keeps one, drop `again`:
        // https://github.com/hyprwm/Hyprland/blob/v0.57.2/src/desktop/view/LayerSurface.cpp#L315-L326
        let again = self.aside.new_reason_over_fullscreen(any_open, fullscreen);
        if held.overlay && (!self.held.overlay || again) {
            // Compositors draw a focused fullscreen window over the Top layer: https://wayland.app/protocols/wlr-layer-shell-unstable-v1
            window.set_layer(Layer::Overlay);
        }
        self.held = held;
        if let Some(focus) = focus {
            window.focus(&focus.unwrap_or_else(|| self.focus.clone()), cx);
        }
        self.aside.set_fullscreen(fullscreen, now);
        self.aside.set_held(any_open, now);
        self.follow_aside(window, cx);

        if catching != self.catching {
            self.catching = catching;
            if let Some(catcher) = self.catcher.take() {
                satellites::close(catcher, cx);
            }
            if catching {
                self.catch_once_settled(window, cx);
            }
        }
        self.fit(window, cx, false);
        cx.notify();
    }

    // Hyprland 0.57 drops a layer change committed before a surface's first frame; once it keeps one, drop this:
    // https://github.com/hyprwm/Hyprland/blob/v0.57.2/src/desktop/view/LayerSurface.cpp#L315-L326
    pub fn ask_layer_once_on_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.layer_asked_on_screen {
            return;
        }
        self.layer_asked_on_screen = true;
        let stage = cx.weak_entity();
        window.on_next_frame(move |window, cx| {
            let overlay = stage.read_with(cx, |stage, _| stage.held.overlay).unwrap_or(false);
            if overlay {
                window.set_layer(Layer::Overlay);
            }
        });
    }

    pub fn rest_once_closed(&mut self, window: &mut Window, cx: &mut App, now: Instant) {
        let drawn =
            self.panels.read(cx).open.any_open(self.screen.id) || self.panel_items().any(|panel| !panel.closed(now));
        if drawn || self.aside.fullscreen() {
            return;
        }
        if self.held.overlay {
            window.set_layer(Layer::Top);
            self.held.overlay = false;
        }
        self.fit(window, cx, true);
    }

    // Opening a window holds every window's frames up while its renderer starts, so never at a morph's start:
    // https://linear.app/imperaluna/issue/IMP-427
    fn catch_once_settled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |stage, cx| {
            let mut waiting = true;
            while waiting {
                cx.background_executor().timer(DURATION_PANEL + DURATION_FADE).await;
                waiting = stage.update(cx, |stage, cx| stage.catch_if_settled(cx)).unwrap_or(false);
            }
        })
        .detach();
    }

    fn catch_if_settled(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.catching || self.catcher.is_some() {
            return false;
        }
        if self.moving(Instant::now()) {
            return true;
        }
        self.catcher = satellites::open_catcher(&self.screen, self.height.clone(), self.beside.clone(), cx);
        false
    }

    fn publish_off_strip(&self) {
        let pill_of = |panel: &PanelItem| {
            let view = panel.from.as_ref()?;
            self.rows.iter().zip(ROWS).find_map(|(row, (_, align))| {
                let pill = row.iter().find(|pill| pill.view == *view)?;
                Some((pill.bounds.clone(), pill.present.clone(), align))
            })
        };
        let off_strip = self.panel_items().filter(|panel| !panel.on_strip());
        *self.off_strip.borrow_mut() =
            off_strip.map(|panel| OffStrip { panel: panel.clone(), pill: pill_of(panel) }).collect();
    }

    fn wants_panel_window(&self) -> bool {
        self.panel_items().any(|panel| !panel.on_strip())
    }

    // Opening a window holds every window's frames up while its renderer starts, so never at a morph's start:
    // https://linear.app/imperaluna/issue/IMP-427
    fn keep_panel_window(&mut self, cx: &mut Context<Self>) {
        if self.wants_panel_window() && self.panel_window.is_none() && !self.panel_window_due {
            self.panel_window_due = true;
            let (screen, beside, frames) = (self.screen.clone(), self.beside.clone(), self.frames.is_some());
            let (stage, panels) = (cx.weak_entity(), self.off_strip.clone());
            cx.defer(move |cx| {
                let shared = panel_window::Shared { stage: stage.clone(), panels, beside };
                let opened = panel_window::open(&screen, shared, frames, cx);
                let kept =
                    stage.update(cx, |stage, cx| stage.panel_window_opened(opened, &screen, cx)).unwrap_or(false);
                if let Some(window) = opened.filter(|_| !kept) {
                    satellites::close(window.into(), cx);
                }
            });
        }
    }

    fn panel_window_opened(
        &mut self,
        opened: Option<WindowHandle<PanelWindow>>,
        screen: &Screen,
        cx: &mut Context<Self>,
    ) -> bool {
        self.panel_window_due = false;
        if screen.size != self.screen.size {
            self.keep_panel_window(cx);
            return false;
        }
        if !self.wants_panel_window() || self.panel_window.is_some() {
            return false;
        }
        self.panel_window = opened;
        self.beside_held = Hold::default();
        let panels = self.panels.read(cx);
        let id = self.screen.id;
        let hold = Hold { keyboard: panels.open.needs_keyboard(id, false), catching: panels.open.catches_clicks() };
        let open_there = |panel: &&PanelItem| !panel.on_strip() && panel.morph.target() > 0.0;
        let focus = self.panel_items().find(open_there).map(|panel| panel.spec.focus.clone());
        self.tell_panel_window(hold, focus.filter(|_| hold.keyboard), true, cx);
        true
    }

    fn close_panel_window(&mut self, cx: &mut App) {
        if let Some(window) = self.panel_window.take() {
            satellites::close(window.into(), cx);
            self.beside.set(px(0.0));
        }
    }

    fn tell_panel_window(&mut self, hold: Hold, focus: Option<Option<FocusHandle>>, moved: bool, cx: &mut App) {
        let Some(panels) = self.panel_window else {
            return;
        };
        if hold == self.beside_held && focus.is_none() && !moved {
            return;
        }
        self.beside_held = hold;
        let told = Told { hold, focus, moved };
        let _ = panels.update(cx, |panels, window, cx| panels.follow(told, window, cx));
    }

    pub fn panel_window_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(catcher) = self.catcher {
            let _ = catcher.update(cx, |_, window, _| window.refresh());
        }
        cx.notify();
    }

    fn fit(&mut self, window: &mut Window, cx: &mut App, may_shrink: bool) {
        let now = Instant::now();
        let drawn = self.panel_items().filter(|panel| panel.on_strip() && !panel.closed(now));
        let wanted = stage_height(drawn.map(|panel| panel.reach(now)));
        let height = self.height.get();
        if wanted == height || (wanted < height && !may_shrink) {
            return;
        }
        self.height.set(wanted);
        window.resize(size(self.screen.size.width, wanted));
        if let Some(catcher) = self.catcher {
            let _ = catcher.update(cx, |_, window, _| window.refresh());
        }
    }

    pub fn follow_aside(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.aside.advance(now);
        let kept = self.aside.room_kept();
        if kept != self.room_kept {
            self.room_kept = kept;
            let said = if kept { "bar space kept" } else { "bar space given to a fullscreen window" };
            eprintln!("INFO nova-ui: {said} on {}", self.screen.name);
            if let Some(room) = self.bar_room {
                satellites::keep_bar_room(room, kept, cx);
            }
        }
        self.aside_wake = self.aside.next_wake().map(|at| {
            cx.spawn_in(window, async move |stage, cx| {
                cx.background_executor().timer(at.saturating_duration_since(Instant::now())).await;
                let _ = stage.update_in(cx, |stage, window, cx| {
                    stage.follow_aside(window, cx);
                    cx.notify();
                });
            })
        });
    }

    fn keep_bar_room(&mut self, cx: &mut App) {
        let has_bar = self.rows.iter().any(|row| !row.is_empty());
        if has_bar && self.bar_room.is_none() {
            self.bar_room = satellites::open_bar_room(&self.screen, self.room_kept, cx);
        } else if !has_bar && let Some(room) = self.bar_room.take() {
            satellites::close(room, cx);
        }
    }

    fn follow_screen(&mut self, screens: Entity<Screens>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(screen) = screens.read(cx).get(self.screen.id).cloned() else {
            return;
        };
        if screen == self.screen {
            return;
        }
        self.screen = screen;
        window.resize(size(self.screen.size.width, self.height.get()));
        self.close_satellites(cx);
        let screen_size = self.screen.size;
        if let Some(beside) = self.panel_window {
            let _ = beside.update(cx, |beside, _, cx| beside.set_screen(screen_size, cx));
        }
        if self.catching {
            self.catch_once_settled(window, cx);
        }
        self.keep_bar_room(cx);
        self.keep_panel_window(cx);
        if self.frames.is_some() {
            self.frames = Some(FrameLog::new(self.screen.name.to_string(), self.screen.refresh, Instant::now()));
        }
        cx.notify();
    }

    pub fn leave(&mut self, cx: &mut App) {
        self.close_satellites(cx);
        self.close_panel_window(cx);
    }

    fn close_satellites(&mut self, cx: &mut App) {
        for window in [self.catcher.take(), self.bar_room.take()].into_iter().flatten() {
            satellites::close(window, cx);
        }
    }
}
