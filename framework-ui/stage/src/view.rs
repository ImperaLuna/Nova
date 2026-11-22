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

use crate::frames::FrameLog;
use crate::geometry::{Align, stage_height};
use crate::item::{Panel, Pill};
use crate::kept::Kept;
use crate::registry;
use crate::reveal::Reveal;
use crate::satellites;
use crate::screens::{Screen, Screens};
use crate::slots::{HOST_BAR_CENTER, HOST_BAR_LEFT, HOST_BAR_RIGHT, HOST_STAGE_PANELS, Slots};
use crate::stage::Panels;
use crate::tokens::STAGE_NAMESPACE;
use gpui::{
    AnyView, AnyWindowHandle, App, AppContext, Bounds, Context, Entity, FocusHandle, Pixels, SharedString, Size,
    Subscription, Window, WindowHandle,
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

/// Where a pill was laid out, shared with the element that finds out.
pub(crate) type PillBox = Rc<Cell<Option<Bounds<Pixels>>>>;

/// A pill on the stage.
pub(crate) struct PillItem {
    pub view: SharedString,
    pub content: AnyView,
    pub opens_on_click: bool,
    /// Where it was laid out in the last frame, in the window.
    pub bounds: PillBox,
    /// 1 shown, 0 hidden under an open panel.
    pub shown: Eased,
    pub panel: Option<PanelItem>,
}

/// A panel on the stage, a pill's own or one from `stage.panels`.
pub(crate) struct PanelItem {
    pub spec: Panel,
    /// The view id of the pill it grows out of.
    pub from: Option<SharedString>,
    /// 0 closed (its pill), 1 open.
    pub morph: Eased,
    pub reveal: Reveal,
    pub width: Eased,
    pub height: Eased,
}

impl PanelItem {
    fn new(spec: Panel, own_pill: Option<&SharedString>, now: Instant) -> Self {
        let resizing = |length: Pixels| Eased::lasting(f32::from(length), DURATION_RESIZE, now);
        Self {
            from: spec.from.clone().or_else(|| own_pill.cloned()),
            morph: Eased::at(0.0, now),
            reveal: Reveal::hidden(now),
            width: resizing(spec.size.width),
            height: resizing(spec.size.height),
            spec,
        }
    }

    fn set_open(&mut self, open: bool, now: Instant) {
        self.reveal.set_open(open, self.morph.value(now), now);
        self.morph.set_target(if open { 1.0 } else { 0.0 }, now);
    }

    fn set_size(&mut self, size: Size<Pixels>, now: Instant) {
        self.width.set_target(f32::from(size.width), now);
        self.height.set_target(f32::from(size.height), now);
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
    /// The window's height, shared with the click catcher below it.
    height: Rc<Cell<Pixels>>,
    held: Held,
    catcher: Option<AnyWindowHandle>,
    bar_room: Option<AnyWindowHandle>,
    _observers: Vec<Subscription>,
}

/// The glass and whether clicks were caught everywhere, as the compositor
/// was last told. `None` before the first frame.
pub(crate) type Sent = Option<(Vec<Shape>, bool)>;

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
                height: Rc::new(Cell::new(height)),
                held: Held::default(),
                catcher: None,
                bar_room: None,
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
            let pill = row.iter().find(|pill| pill.view == *view)?;
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
                let Pill { content, panel, opens_on_click } = registry::build_pill(view, host, &screen, cx)?;
                Some(PillItem {
                    view: view.clone(),
                    content,
                    opens_on_click,
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
        self.follow_panels(window, cx);
        // A stage that lost its tallest panel gets shorter again.
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
        let held = Held { keyboard: panels.open.needs_keyboard(id), overlay: panels.open.any_open(id) };
        let catching = panels.open.catches_clicks();
        let targets: Vec<(bool, Option<Size<Pixels>>)> = self
            .panel_items()
            .map(|panel| (panels.open.is_open(&panel.spec.name, id), panels.size(&panel.spec.name, id)))
            .collect();
        let mut focus = None;
        for (panel, (open, size)) in self.panel_items_mut().zip(targets) {
            let opened = open && panel.morph.target() == 0.0;
            panel.set_open(open, now);
            if let Some(size) = size {
                panel.set_size(size, now);
            }
            if opened && panel.spec.rules().closes_on_esc() {
                focus = Some(panel.spec.focus.clone());
            }
        }

        if held.keyboard != self.held.keyboard {
            let keyboard = if held.keyboard { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None };
            window.set_keyboard_interactivity(keyboard);
        }
        if held.overlay != self.held.overlay {
            // Over a fullscreen window while a panel is open: compositors
            // draw a focused fullscreen window over the top layer.
            window.set_layer(if held.overlay { Layer::Overlay } else { Layer::Top });
        }
        self.held = held;
        if let Some(focus) = focus {
            window.focus(&focus.unwrap_or_else(|| self.focus.clone()), cx);
        }

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
        self.catcher = satellites::open_catcher(&self.screen, self.height.clone(), cx);
        false
    }

    /// Makes the window as tall as its tallest panel reaches. It grows at
    /// once, before the panel does; it shrinks only when `may_shrink`, when
    /// the panels changed, so a resize never costs a second configure.
    fn fit(&mut self, window: &mut Window, cx: &mut App, may_shrink: bool) {
        let now = Instant::now();
        let wanted = stage_height(self.panel_items().map(|panel| panel.reach(now)));
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

    /// Keeps the bar's room while any of its rows has a pill.
    fn keep_bar_room(&mut self, cx: &mut App) {
        let has_bar = self.rows.iter().any(|row| !row.is_empty());
        if has_bar && self.bar_room.is_none() {
            self.bar_room = satellites::open_bar_room(&self.screen, cx);
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
    }
}
