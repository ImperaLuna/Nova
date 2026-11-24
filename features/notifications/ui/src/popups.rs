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

const QUICK_SETTINGS: &str = "quick-settings";
const BAR: &str = "bar";
const CLOCK_TICK: Duration = Duration::from_secs(60);

pub type Heights = Rc<RefCell<HashMap<u64, Pixels>>>;

pub struct Popups {
    model: Entity<Model>,
    stack: Stack,
    target: Option<DisplayId>,
    windows: Vec<(DisplayId, WindowHandle<PopupWindow>)>,
    opening: Vec<DisplayId>,
    failed: Vec<DisplayId>,
    heights: Heights,
    desktop: Entity<Live<Desktop>>,
    features: Entity<Live<Vec<FeatureState>>>,
    list_open: bool,
    list_on: Vec<DisplayId>,
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
                    popups.follow_list(cx);
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
                list_on: Vec::new(),
                _followed: followed,
            }
        });
        popups.update(cx, |popups, cx| popups.sync_windows(cx));
        popups
    }

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
        let lost = self.target.is_none_or(|target| !here.iter().any(|screen| screen.id == target));
        if lost && self.stack.busy() {
            self.target = self.focused_screen(cx);
            cx.notify();
        }
    }

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
        if !self.target.is_some_and(|target| self.has_window(target)) {
            return;
        }
        let now = Instant::now();
        eprintln!("INFO nova-ui: notification {} pops up", notification.id);
        self.stack.add(notification, now);
        self.stack.sync(&self.model.read(cx).popups(cx), now);
        cx.notify();
    }

    fn has_window(&self, screen: DisplayId) -> bool {
        self.windows.iter().any(|(id, _)| *id == screen) || self.opening.contains(&screen)
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let items = self.model.read(cx).popups(cx);
        self.stack.sync(&items, Instant::now());
        cx.notify();
    }

    fn follow_list(&mut self, cx: &mut Context<Self>) {
        let on: Vec<DisplayId> = Stage::screens(cx)
            .map(|screens| {
                let shown = |screen: &&Screen| {
                    Stage::is_open(QUICK_SETTINGS, screen.id, cx)
                        && Stage::page(QUICK_SETTINGS, screen.id, cx).is_none()
                };
                screens.read(cx).list().iter().filter(shown).map(|screen| screen.id).collect()
            })
            .unwrap_or_default();
        if on == self.list_on {
            return;
        }
        let moved = crate::logic::list_moved(&self.list_on, &on);
        self.list_on = on;
        let open = moved.pauses_popups;
        if moved.read_where_it_left && crate::logic::unread(self.model.read(cx).items(cx)) > 0 {
            model::read_all(cx);
        }
        if open != self.list_open {
            self.list_open = open;
            if open {
                self.stack.clear(Instant::now());
            }
            cx.notify();
        }
    }

    pub fn top(&self, cx: &App) -> Pixels {
        let features = self.features.read(cx).get();
        let bar = features.is_some_and(|features| {
            features.iter().any(|feature| feature.id == BAR && feature.status == FeatureStatus::Ready)
        });
        if bar { BAR_GAP + BAR_HEIGHT + GAP } else { BAR_MARGIN }
    }

    pub fn step(&mut self, now: Instant) {
        self.stack.step(now);
        let entries = self.stack.entries();
        self.heights.borrow_mut().retain(|key, _| entries.iter().any(|entry| entry.key == *key));
    }

    pub fn open_arrivals(&mut self, now: Instant) {
        self.stack.open_arrivals(now);
    }

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

pub enum Act {
    Open,
    Dismiss,
    Button(String),
}

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
