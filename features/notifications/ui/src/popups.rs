//! The pop-ups: a window of their own on each screen, on the overlay layer
//! under the right pill, taking no keyboard and no space, with clicks and blur
//! only on the cards, so they show over fullscreen windows too. The stack comes
//! out on the screen that had focus when the first of them came, and stays
//! there until they've all gone. None while quick settings is open: it shows
//! the list.
//!
//! Each screen's window opens when the overlay starts, never at the start
//! of a motion (opening one stalls every window's frames), and is only as
//! big as the stack plus its shadow. Only the window on the screen they
//! came out on draws, and it asks for frames only while a card moves; a
//! card's time drains in slow steps between its frames.

use crate::model::{self, Model, Popped};
use crate::stack::{Leaving, Stack};
use crate::window::PopupWindow;
use gpui::{App, AppContext, Context, DisplayId, Entity, Pixels, SharedString, Subscription, WeakEntity, WindowHandle};
use nova_daemon_info::{Event as DaemonEvent, FeatureState, FeatureStatus, topics};
use nova_desktop_info::{Desktop, Event as DesktopEvent};
use nova_look::tokens::{BAR_GAP, BAR_HEIGHT, BAR_MARGIN, GAP};
use nova_notifications_api::Notification;
use nova_stage::{Screen, Stage};
use nova_ui_client::Live;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The feature whose panel shows the list, and the bar's: with the bar
/// off, the stack sits at the top.
const QUICK_SETTINGS: &str = "quick-settings";
const BAR: &str = "bar";
/// "5m" moves on while pop-ups are up.
const CLOCK_TICK: Duration = Duration::from_secs(60);

/// Each card's height as it was last laid out, by entry, which the slots
/// below it are placed by.
pub type Heights = Rc<RefCell<HashMap<u64, Pixels>>>;

pub struct Popups {
    model: Entity<Model>,
    stack: Stack,
    /// The screen they come out on, while any are up.
    target: Option<DisplayId>,
    windows: Vec<(DisplayId, WindowHandle<PopupWindow>)>,
    /// Screens whose window is about to open, and those whose window
    /// couldn't open (tried again when the screens change).
    opening: Vec<DisplayId>,
    failed: Vec<DisplayId>,
    heights: Heights,
    desktop: Entity<Live<Desktop>>,
    features: Entity<Live<Vec<FeatureState>>>,
    /// Quick settings is open on some screen.
    list_open: bool,
    _followed: Vec<Subscription>,
}

impl Popups {
    pub fn start(cx: &mut App) -> Entity<Self> {
        let popups = cx.new(|cx: &mut Context<Self>| {
            let model = Model::get(cx);
            let desktop = Live::follow(nova_desktop_info::TOPIC, cx, |DesktopEvent::Compositor(desktop)| Some(desktop));
            let features = Live::follow(topics::FEATURES, cx, |event| match event {
                DaemonEvent::Features { features } => Some(features),
                _ => None,
            });
            let mut followed = vec![
                cx.subscribe(&model, |popups: &mut Self, _, Popped(notification), cx| {
                    popups.pop_up(notification.clone(), cx);
                }),
                cx.observe(&model, |popups: &mut Self, _, cx| popups.sync(cx)),
                cx.observe(&features, |_, _, cx| cx.notify()),
                cx.on_release(|popups: &mut Self, cx| popups.close_windows(cx)),
            ];
            if let Some(screens) = Stage::screens(cx) {
                followed.push(cx.observe(&screens, |popups: &mut Self, _, cx| {
                    popups.failed.clear();
                    popups.sync_windows(cx);
                }));
            }
            let this = cx.weak_entity();
            if let Some(open) = Stage::observe_open(cx, move |cx| {
                let _ = this.update(cx, |popups, cx| popups.follow_list(cx));
            }) {
                followed.push(open);
            }
            Self::keep_clock(cx);
            Self {
                model,
                stack: Stack::default(),
                target: None,
                windows: Vec::new(),
                opening: Vec::new(),
                failed: Vec::new(),
                heights: Heights::default(),
                desktop,
                features,
                list_open: false,
                _followed: followed,
            }
        });
        popups.update(cx, |popups, cx| popups.sync_windows(cx));
        popups
    }

    /// "5m" moves on while pop-ups are up.
    fn keep_clock(cx: &mut Context<Self>) {
        cx.spawn(async move |popups, cx| {
            let mut running = true;
            while running {
                cx.background_executor().timer(CLOCK_TICK).await;
                running = popups.update(cx, |popups, cx| popups.clock_tick(cx)).is_ok();
            }
        })
        .detach();
    }

    fn clock_tick(&mut self, cx: &mut Context<Self>) {
        if self.stack.busy() {
            cx.notify();
        }
    }

    /// Opens a window on each screen that has none, and closes those whose
    /// screen left.
    fn sync_windows(&mut self, cx: &mut Context<Self>) {
        let Some(screens) = Stage::screens(cx) else { return };
        let here = screens.read(cx).list().to_vec();
        let mut windows = std::mem::take(&mut self.windows);
        windows.retain(|(screen, window)| {
            let kept = here.iter().any(|kept| kept.id == *screen) && window.update(cx, |_, _, _| ()).is_ok();
            if !kept {
                let _ = window.update(cx, |_, window, _| window.remove_window());
            }
            kept
        });
        self.windows = windows;
        let new: Vec<Screen> = here
            .iter()
            .filter(|screen| {
                !self.windows.iter().any(|(id, _)| *id == screen.id)
                    && !self.opening.contains(&screen.id)
                    && !self.failed.contains(&screen.id)
            })
            .cloned()
            .collect();
        if !new.is_empty() {
            // Opened after this update: a new window draws at once, and its
            // drawing reads the pop-ups.
            self.opening.extend(new.iter().map(|screen| screen.id));
            let (this, heights) = (cx.weak_entity(), self.heights.clone());
            cx.defer(move |cx| {
                let opened: Vec<_> = new
                    .iter()
                    .map(|screen| (screen.id, PopupWindow::open(screen, this.clone(), heights.clone(), cx)))
                    .collect();
                let _ = this.update(cx, |popups, cx| popups.opened(opened, cx));
            });
        }
        // Those still up move to the screen with focus when theirs left,
        // or when every screen had gone and one came back.
        let lost = self.target.is_none_or(|target| !here.iter().any(|screen| screen.id == target));
        if lost && self.stack.busy() {
            self.target = self.focused_screen(cx);
            cx.notify();
        }
    }

    /// The windows a deferred open made, by screen; then the screens are
    /// looked at again, in case they changed meanwhile.
    fn opened(&mut self, opened: Vec<(DisplayId, Option<WindowHandle<PopupWindow>>)>, cx: &mut Context<Self>) {
        for (screen, window) in opened {
            self.opening.retain(|id| *id != screen);
            match window {
                Some(window) => self.windows.push((screen, window)),
                None => self.failed.push(screen),
            }
        }
        self.sync_windows(cx);
    }

    fn close_windows(&mut self, cx: &mut App) {
        for (_, window) in self.windows.drain(..) {
            let _ = window.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// The screen with focus, or the first.
    fn focused_screen(&self, cx: &App) -> Option<DisplayId> {
        let desktop = self.desktop.read(cx).get();
        let output = desktop.and_then(|desktop| desktop.outputs.iter().find(|output| output.focused));
        let screens = Stage::screens(cx)?;
        screens.read(cx).for_output(output.map(|output| output.name.as_str())).map(|screen| screen.id)
    }

    fn pop_up(&mut self, notification: Notification, cx: &mut Context<Self>) {
        if self.list_open {
            return;
        }
        if !self.stack.busy() || self.target.is_none() {
            self.target = self.focused_screen(cx);
        }
        // Only that screen's window moves the stack on: without one (it
        // couldn't open) nothing would ever take the pop-up off again.
        if !self.target.is_some_and(|target| self.has_window(target)) {
            return;
        }
        let now = Instant::now();
        eprintln!("INFO nova-ui: notification {} pops up", notification.id);
        self.stack.add(notification, now);
        // The list came first: a pop-up already in it is marked listed now,
        // so it goes when the list next loses it.
        self.stack.sync(&self.model.read(cx).popups(cx), now);
        // The window opens it, with the others that came meanwhile, once
        // it has grown to hold them.
        cx.notify();
    }

    /// `screen` has a window, or is about to.
    fn has_window(&self, screen: DisplayId) -> bool {
        self.windows.iter().any(|(id, _)| *id == screen) || self.opening.contains(&screen)
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let items = self.model.read(cx).popups(cx);
        self.stack.sync(&items, Instant::now());
        cx.notify();
    }

    /// Quick settings opened or closed: its list has the pop-ups meanwhile,
    /// and what it showed is read once it's gone.
    fn follow_list(&mut self, cx: &mut Context<Self>) {
        let open = Stage::screens(cx)
            .map(|screens| screens.read(cx).list().iter().any(|screen| Stage::is_open(QUICK_SETTINGS, screen.id, cx)))
            .unwrap_or(false);
        if open == self.list_open {
            return;
        }
        self.list_open = open;
        if open {
            self.stack.clear(Instant::now());
        } else if crate::logic::unread(self.model.read(cx).items(cx)) > 0 {
            model::read_all(cx);
        }
        cx.notify();
    }

    /// Where the stack sits: under the bar, the room above it being where
    /// the pop-ups grow out of the quick settings button; with the bar off,
    /// at the top, as far in as from the side.
    pub fn top(&self, cx: &App) -> Pixels {
        let features = self.features.read(cx).get();
        let bar = features.is_some_and(|features| {
            features.iter().any(|feature| feature.id == BAR && feature.status == FeatureStatus::Ready)
        });
        if bar { BAR_GAP + BAR_HEIGHT + GAP } else { BAR_MARGIN }
    }

    /// Moves the stack on to `now`, for the window that draws it.
    pub fn step(&mut self, now: Instant) {
        self.stack.step(now);
        let entries = self.stack.entries();
        self.heights.borrow_mut().retain(|key, _| entries.iter().any(|entry| entry.key == *key));
    }

    /// The new ones open, now that the window that draws them holds them.
    pub fn open_arrivals(&mut self, now: Instant) {
        self.stack.open_arrivals(now);
    }

    /// Whether `screen`'s window has anything to draw: the pop-ups are up,
    /// and came out there.
    pub fn shows_on(&self, screen: DisplayId) -> bool {
        self.target == Some(screen) && self.stack.busy()
    }

    pub fn stack(&self) -> &Stack {
        &self.stack
    }

    fn act(&mut self, key: u64, output: &SharedString, act: Act, cx: &mut Context<Self>) {
        let now = Instant::now();
        let Some(entry) = self.stack.entries().iter().find(|entry| entry.key == key && entry.open(now)) else {
            return;
        };
        let notification = entry.notification.clone();
        match act {
            Act::Open => model::open(&notification, output, cx),
            Act::Dismiss => model::dismiss(notification.id, cx),
            Act::Button(action) => model::act(notification.id, &action, cx),
        }
        self.stack.remove(key, Leaving::Away, now);
        cx.notify();
    }

    fn hover(&mut self, key: u64, held: bool, cx: &mut Context<Self>) {
        self.stack.set_held(key, held, Instant::now());
        cx.notify();
    }
}

/// What a click on a card does.
pub enum Act {
    Open,
    Dismiss,
    Button(String),
}

/// How a card reaches the pop-ups, and the screen it's drawn on (where the
/// app it opens should show).
#[derive(Clone)]
pub struct Reach {
    popups: WeakEntity<Popups>,
    output: SharedString,
}

impl Reach {
    pub fn new(popups: WeakEntity<Popups>, output: SharedString) -> Self {
        Self { popups, output }
    }

    pub fn act(&self, key: u64, act: Act, cx: &mut App) {
        let _ = self.popups.update(cx, |popups, cx| popups.act(key, &self.output, act, cx));
    }

    pub fn hover(&self, key: u64, held: bool, cx: &mut App) {
        let _ = self.popups.update(cx, |popups, cx| popups.hover(key, held, cx));
    }
}
