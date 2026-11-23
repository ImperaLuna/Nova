//! One screen's stage: the window, what is on it, and how that follows the
//! features and the open panels. `draw.rs` renders it.
//!
//! What is on it comes from the slots: a [`PillItem`] per `bar.*` slot, in
//! three rows, and a [`PanelItem`] per `stage.panels` slot and per pill
//! with a panel. Each is built once, when its feature becomes ready, and
//! kept while it stays. The view observes the shared [`Panels`] and turns
//! what it says into targets (a panel's morph heads for open or closed),
//! the window's keyboard and layer, and the click catcher; rendering only
//! reads where things are right now.
//!
//! A panel placed off the strip (`Placement`) is kept here like any other,
//! and drawn by the window beside this one (`crate::panel_window`), which
//! this view opens while it has such a panel and tells what to hold.

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
    Subscription, Task, Window, WindowHandle,
    layer_shell::{KeyboardInteractivity, Layer},
    px, size,
};
use nova_look::motion::Eased;
use nova_look::tokens::{DURATION_FADE, DURATION_FAST, DURATION_PANEL, DURATION_RESIZE};
use nova_look::{Shape, ShellWindow};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

/// The bar's rows, with the host each is filled from and the way its
/// pills' panels keep to them.
pub(crate) const ROWS: [(&str, Align); 3] =
    [(HOST_BAR_LEFT, Align::Left), (HOST_BAR_CENTER, Align::Center), (HOST_BAR_RIGHT, Align::Right)];

/// The center row's place in [`ROWS`].
pub(crate) const CENTER_ROW: usize = 1;

/// Where a pill was laid out, shared with the element that finds out.
pub(crate) type PillBox = Rc<Cell<Option<Bounds<Pixels>>>>;

/// A pill on the stage.
pub(crate) struct PillItem {
    pub view: SharedString,
    pub content: AnyView,
    pub opens_on_click: bool,
    /// Whether its view has something to show (`Pill::shown_while`).
    pub present: Option<Shown>,
    /// Where it was laid out in the last frame, in the window.
    pub bounds: PillBox,
    /// 1 shown, 0 hidden under an open panel.
    pub shown: Eased,
    pub panel: Option<PanelItem>,
}

impl PillItem {
    /// Whether it is drawn at all: not while its view has nothing to show.
    pub fn is_present(&self) -> bool {
        self.present.as_ref().is_none_or(Shown::get)
    }
}

/// A panel on the stage, a pill's own or one from `stage.panels`.
#[derive(Clone)]
pub(crate) struct PanelItem {
    pub spec: Panel,
    /// The view id of the pill it grows out of.
    pub from: Option<SharedString>,
    /// 0 closed (its pill), 1 open.
    pub morph: Eased,
    pub reveal: Reveal,
    pub width: Eased,
    pub height: Eased,
    /// Off the strip: whether the panels' window has it on screen. The
    /// pill it grows out of is on the strip, and two windows never draw in
    /// the same frame, so the pill stays until the panel is drawn over it,
    /// and the panel stays, closed, until the pill is drawn again
    /// (`covering` in today's shell).
    pub cover: Rc<Cell<bool>>,
}

impl PanelItem {
    fn new(spec: Panel, own_pill: Option<&SharedString>, now: Instant) -> Self {
        let resizing = |length: Pixels| Eased::lasting(f32::from(length), DURATION_RESIZE, now);
        let grows = spec.placement != Placement::WholeScreen;
        Self {
            from: spec.from.clone().or_else(|| own_pill.cloned()).filter(|_| grows),
            morph: Eased::at(0.0, now),
            reveal: Reveal::hidden(now),
            width: resizing(spec.size.width),
            height: resizing(spec.size.height),
            cover: Rc::default(),
            spec,
        }
    }

    /// Whether the strip draws it; the panels' window draws the others.
    pub fn on_strip(&self) -> bool {
        self.spec.placement.on_strip()
    }

    /// Returns whether that turned it round.
    fn set_open(&mut self, open: bool, now: Instant) -> bool {
        let target = if open { 1.0 } else { 0.0 };
        let turned = self.morph.target() != target;
        self.reveal.set_open(open, self.morph.value(now), now);
        self.morph.set_target(target, now);
        turned
    }

    /// Returns whether it heads for another size now.
    fn set_size(&mut self, size: Size<Pixels>, now: Instant) -> bool {
        let (width, height) = (f32::from(size.width), f32::from(size.height));
        let changed = self.width.target() != width || self.height.target() != height;
        self.width.set_target(width, now);
        self.height.set_target(height, now);
        changed
    }

    /// Whether it is closed and at rest: nothing of it is drawn.
    pub fn closed(&self, now: Instant) -> bool {
        self.morph.target() == 0.0 && self.morph.settled(now)
    }

    pub fn moving(&self, now: Instant) -> bool {
        !(self.morph.settled(now) && self.reveal.settled(now) && self.width.settled(now) && self.height.settled(now))
    }

    /// How far down the screen it reaches once open: at the size it is
    /// heading for, or the one it still has while it eases to a smaller.
    fn reach(&self, now: Instant) -> Pixels {
        self.spec.top + px(self.height.target().max(self.height.value(now)))
    }
}

/// A panel placed off the strip as the panels' window draws it: where it
/// is heading since the stage last followed the shared state, and the
/// pill it grows out of (its box, whether it is there, the way its row
/// keeps).
pub(crate) struct OffStrip {
    pub panel: PanelItem,
    pub pill: Option<(PillBox, Option<Shown>, Align)>,
}

impl OffStrip {
    /// Where its pill is, like `StageView::pill_box`.
    pub fn pill_box(&self) -> Option<(Bounds<Pixels>, Align)> {
        let (bounds, present, align) = self.pill.as_ref()?;
        let there = present.as_ref().is_none_or(Shown::get);
        bounds.get().filter(|_| there).map(|bounds| (bounds, *align))
    }
}

/// The panels placed off the strip, which the stage's view writes and the
/// panels' window reads. Not the view itself: GPUI counts a view read
/// while a window draws as that window's, and hands the view's callbacks
/// that window from then on (`App::current_window_by_entity`), so the
/// stage would resize the panels' window in place of its own.
pub(crate) type OffStripPanels = Rc<RefCell<Vec<OffStrip>>>;

/// What the window was last told to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Held {
    keyboard: bool,
    overlay: bool,
}

pub(crate) struct StageView {
    pub screen: Screen,
    pub panels: Entity<Panels>,
    slots: Entity<Slots>,
    /// The pills of the left, center and right row.
    pub rows: [Kept<PillItem>; 3],
    /// The panels that are no pill's own.
    pub standalone: Kept<PanelItem>,
    /// The glass drawn in the frame under way, and what the compositor was
    /// last told of it (`draw.rs`).
    pub shapes: Rc<RefCell<Vec<Shape>>>,
    pub sent: Rc<RefCell<Sent>>,
    /// Whether every screen takes clicks everywhere right now.
    pub catching: bool,
    /// Gets the keyboard for a panel that names no field of its own, so Esc
    /// arrives.
    pub focus: FocusHandle,
    pub frames: Option<FrameLog>,
    /// How far left of the center row's right edge the point it keeps at
    /// the screen's middle is (`middle.rs`), when a view marks one.
    pub middle_from_right: Option<Pixels>,
    /// Whether the center row's content is drawn in the frame under way,
    /// so a mark is to be expected from it.
    pub center_drawn: bool,
    /// The window's height, shared with the click catcher below it.
    height: Rc<Cell<Pixels>>,
    held: Held,
    /// The window beside this one for the panels placed off the strip,
    /// while this screen has one; what it was last told to hold; and how
    /// wide it is on screen (nothing while none of its panels is out),
    /// which it says here for this window and the click catcher to leave
    /// it its clicks.
    pub panel_window: Option<WindowHandle<PanelWindow>>,
    panel_window_due: bool,
    beside_held: Hold,
    pub beside: Rc<Cell<Pixels>>,
    /// The panels that window draws, as of the last `follow_panels`.
    off_strip: OffStripPanels,
    catcher: Option<AnyWindowHandle>,
    bar_room: Option<AnyWindowHandle>,
    /// The bar stepping aside for a fullscreen window, and the wake-up for
    /// what it has due next.
    pub aside: Aside,
    aside_wake: Option<Task<()>>,
    /// Whether the bar's room was last told to keep its room.
    room_kept: bool,
    _observers: Vec<Subscription>,
}

/// The glass, whether clicks were caught everywhere, whether the strip
/// along the top edge took the pointer and how wide the panels' window
/// beside was, as the compositor was last told. `None` before the first
/// frame.
pub(crate) type Sent = Option<(Vec<Shape>, bool, bool, Pixels)>;

/// What every screen's stage is opened with.
pub(crate) struct Shared {
    pub panels: Entity<Panels>,
    pub slots: Entity<Slots>,
    pub screens: Entity<Screens>,
    pub log_frames: bool,
}

/// Opens the stage on `screen`.
pub(crate) fn open(screen: &Screen, shared: &Shared, cx: &mut App) -> Option<WindowHandle<StageView>> {
    let Shared { panels, slots, screens, log_frames } = shared;
    let log_frames = *log_frames;
    let height = stage_height([]);
    let mut shell = ShellWindow::new(screen.id, size(screen.size.width, height));
    shell.namespace = STAGE_NAMESPACE;
    let (screen, panels, slots, screens) = (screen.clone(), panels.clone(), slots.clone(), screens.clone());
    let opened = cx.open_window(shell.options(), move |window, cx| {
        cx.new(|cx| {
            let now = Instant::now();
            let observers = vec![
                cx.observe_in(&slots, window, |stage: &mut StageView, _, window, cx| stage.sync_items(window, cx)),
                cx.observe_in(&panels, window, |stage: &mut StageView, _, window, cx| stage.follow_panels(window, cx)),
                cx.observe_in(&screens, window, StageView::follow_screen),
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
                panel_window: None,
                panel_window_due: false,
                beside_held: Hold::default(),
                beside: Rc::default(),
                off_strip: Rc::default(),
                catcher: None,
                bar_room: None,
                aside: Aside::new(now),
                aside_wake: None,
                room_kept: true,
                _observers: observers,
            };
            stage.sync_items(window, cx);
            stage
        })
    });
    match opened {
        Ok(window) => Some(window),
        Err(error) => {
            eprintln!("ERROR nova-ui: can't open the stage: {error:#}");
            None
        }
    }
}

impl StageView {
    /// Every panel on this stage.
    pub fn panel_items(&self) -> impl Iterator<Item = &PanelItem> {
        let of_pills = self.rows.iter().flat_map(|row| row.iter()).filter_map(|pill| pill.panel.as_ref());
        of_pills.chain(self.standalone.iter())
    }

    fn panel_items_mut(&mut self) -> impl Iterator<Item = &mut PanelItem> {
        let of_pills = self.rows.iter_mut().flat_map(|row| row.iter_mut()).filter_map(|pill| pill.panel.as_mut());
        of_pills.chain(self.standalone.iter_mut())
    }

    /// Where the pill with view id `view` is, and the way its row keeps.
    pub fn pill_box(&self, view: &SharedString) -> Option<(Bounds<Pixels>, Align)> {
        self.rows.iter().zip(ROWS).find_map(|(row, (_, align))| {
            let pill = row.iter().find(|pill| pill.view == *view && pill.is_present())?;
            Some((pill.bounds.get()?, align))
        })
    }

    /// Follows the slots: builds the pills and panels that are new, drops
    /// those whose feature left, and tells the shared state which panels
    /// this screen has.
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
        // A stage that lost the panel it was sized for gets shorter again.
        self.fit(window, cx, true);
        self.keep_bar_room(cx);
        cx.notify();
    }

    /// Follows the shared state: each panel heads for open or closed and
    /// for its size, the window takes or gives back the keyboard and its
    /// layer, and the click catcher comes or goes.
    fn follow_panels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let id = self.screen.id;
        let panels = self.panels.read(cx);
        // Raised while a panel is open, and until the last one has closed
        // all the way (`rest_once_closed`); and while the bar steps aside
        // for a fullscreen window, so touching the top edge brings the pills
        // back over it. Hyprland gives a Top layer under a focused
        // fullscreen window no pixels and no pointer (IMP-182).
        let has_bar = self.rows.iter().any(|row| !row.is_empty());
        let fullscreen = has_bar && panels.fullscreen(&self.screen.name);
        let any_open = panels.open.any_open(id);
        let overlay = any_open || self.held.overlay || fullscreen;
        let held = Held { keyboard: panels.open.needs_keyboard(id, true), overlay };
        let catching = panels.open.catches_clicks();
        let beside = Hold { keyboard: panels.open.needs_keyboard(id, false), catching };
        let targets: Vec<(bool, Option<Size<Pixels>>)> = self
            .panel_items()
            .map(|panel| (panels.open.is_open(&panel.spec.name, id), panels.size(&panel.spec.name, id)))
            .collect();
        // The field that gets the keyboard, in this window and in the one
        // beside it, and whether a panel drawn there heads elsewhere now.
        let (mut focus, mut beside_focus, mut beside_moved) = (None, None, false);
        for (panel, (open, size)) in self.panel_items_mut().zip(targets) {
            let opened = open && panel.morph.target() == 0.0;
            let moved = panel.set_open(open, now) | size.is_some_and(|size| panel.set_size(size, now));
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
        if held.overlay && !self.held.overlay {
            // Over a fullscreen window while a panel is open: compositors
            // draw a focused fullscreen window over the top layer.
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

    /// Once no panel is drawn any more: back under fullscreen windows (a
    /// panel that is closing is still over the window it opened over), and
    /// as short as the bar again.
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

    /// Opens the click catcher once nothing on this stage moves any more.
    /// A new window sets up its own drawing, which holds every window's
    /// frames up for a moment: at once, that moment would be the start of
    /// the panel's morph. Until then a click below the stage still reaches
    /// the window there.
    fn catch_once_settled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |stage, cx| {
            let mut waiting = true;
            while waiting {
                // Long enough for a panel on any screen to open: the other
                // screens' stages stand still meanwhile.
                cx.background_executor().timer(DURATION_PANEL + DURATION_FADE).await;
                waiting = stage.update(cx, |stage, cx| stage.catch_if_settled(cx)).unwrap_or(false);
            }
        })
        .detach();
    }

    /// Returns whether to look again later.
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

    /// Writes the panels placed off the strip where the panels' window
    /// reads them (`OffStripPanels`).
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

    /// Keeps the window for the panels placed off the strip while this
    /// screen has such a panel. It is opened here, when a feature becomes
    /// ready, and kept: a new window holds every window's frames up for a
    /// moment, which must never be the start of a panel's morph. It opens
    /// once this update is over: a window draws its first frame as it
    /// opens, and tells this view when it changes size.
    fn keep_panel_window(&mut self, cx: &mut Context<Self>) {
        if !self.wants_panel_window() {
            self.close_panel_window(cx);
        } else if self.panel_window.is_none() && !self.panel_window_due {
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

    /// Takes the window opened for the panels off the strip, and tells it
    /// what is open already. Returns whether it is kept: not when the
    /// panels it was for left meanwhile, or the screen changed again.
    fn panel_window_opened(
        &mut self,
        opened: Option<WindowHandle<PanelWindow>>,
        screen: &Screen,
        cx: &mut Context<Self>,
    ) -> bool {
        self.panel_window_due = false;
        if screen.size != self.screen.size {
            // Opened for the screen as it was: one for what it is now.
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

    /// Tells the panels' window what to hold, which field gets the keyboard
    /// (`focus`, for a panel that opened there), and to draw again when
    /// one of its panels heads elsewhere (`moved`).
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

    /// The panels' window changed its size, or drew a panel for the first
    /// time: the click regions here and the click catcher's leave it its
    /// own, and a pill it drew over steps aside.
    pub fn panel_window_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(catcher) = self.catcher {
            let _ = catcher.update(cx, |_, window, _| window.refresh());
        }
        cx.notify();
    }

    /// Makes the window as tall as the tallest panel drawn on it reaches
    /// (one placed off the strip is drawn in the window beside, and never
    /// makes this one taller),
    /// and as short as the bar with none: a compositor's work behind the
    /// stage grows with the surface. It grows at once, before the panel
    /// does; it shrinks only when `may_shrink`, once nothing is drawn or
    /// the panels changed, so one panel taking over from another costs no
    /// resize in between.
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
            // Its region ends where the stage begins.
            let _ = catcher.update(cx, |_, window, _| window.refresh());
        }
    }

    /// Hands on what stepping aside changed: the bar's room, and a wake-up
    /// for what is due next.
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

    /// Keeps the bar's room while any of its rows has a pill.
    fn keep_bar_room(&mut self, cx: &mut App) {
        let has_bar = self.rows.iter().any(|row| !row.is_empty());
        if has_bar && self.bar_room.is_none() {
            self.bar_room = satellites::open_bar_room(&self.screen, self.room_kept, cx);
        } else if !has_bar && let Some(room) = self.bar_room.take() {
            satellites::close(room, cx);
        }
    }

    /// The screen changed its size (a new mode): lay out by the new one.
    fn follow_screen(&mut self, screens: Entity<Screens>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(screen) = screens.read(cx).get(self.screen.id).cloned() else {
            // It left: the stage closes this window next.
            return;
        };
        if screen == self.screen {
            return;
        }
        self.screen = screen;
        window.resize(size(self.screen.size.width, self.height.get()));
        // The windows beside this one were opened at the old size.
        self.leave(cx);
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

    /// Closes the windows beside this one: the screen left, or changed.
    pub fn leave(&mut self, cx: &mut App) {
        for window in [self.catcher.take(), self.bar_room.take()].into_iter().flatten() {
            satellites::close(window, cx);
        }
        self.close_panel_window(cx);
    }
}
