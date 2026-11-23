use crate::item::{Panel, Part, Pill};
use crate::kept::Kept;
use crate::screens::Screen;
use crate::slots::Slots;
use crate::stage::Stage;
use gpui::{AnyView, App, Entity, Global, SharedString};
use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;

type Build<T> = Rc<dyn Fn(&Screen, &mut App) -> T>;
type BuildOverlay = Rc<dyn Fn(&mut App) -> Box<dyn Any>>;

#[derive(Default)]
struct Registry {
    pills: HashMap<SharedString, Build<Pill>>,
    panels: HashMap<SharedString, Build<Panel>>,
    parts: HashMap<SharedString, Build<Part>>,
    overlays: HashMap<SharedString, BuildOverlay>,
}

impl Global for Registry {}

pub fn pill(view: &'static str, cx: &mut App, build: impl Fn(&Screen, &mut App) -> Pill + 'static) {
    cx.default_global::<Registry>().pills.insert(view.into(), Rc::new(build));
}

pub fn panel(view: &'static str, cx: &mut App, build: impl Fn(&Screen, &mut App) -> Panel + 'static) {
    cx.default_global::<Registry>().panels.insert(view.into(), Rc::new(build));
}

pub fn part(view: &'static str, cx: &mut App, build: impl Fn(&Screen, &mut App) -> Part + 'static) {
    cx.default_global::<Registry>().parts.insert(view.into(), Rc::new(build));
}

pub fn has_part(view: &str, cx: &App) -> bool {
    cx.try_global::<Registry>().is_some_and(|registry| registry.parts.contains_key(view))
}

pub fn overlay(view: &'static str, cx: &mut App, build: impl Fn(&mut App) -> Box<dyn Any> + 'static) {
    cx.default_global::<Registry>().overlays.insert(view.into(), Rc::new(build));
}

fn build<F: Clone, T>(
    view: &SharedString,
    host: &str,
    cx: &mut App,
    find: impl FnOnce(&Registry) -> Option<&F>,
    run: impl FnOnce(F, &mut App) -> T,
) -> Option<T> {
    let found = cx.try_global::<Registry>().and_then(find).cloned();
    if found.is_none() {
        eprintln!("WARN nova-ui: no view {view} for {host}: its feature's ui didn't register it");
    }
    found.map(|build| run(build, cx))
}

pub(crate) fn build_pill(view: &SharedString, host: &str, screen: &Screen, cx: &mut App) -> Option<Pill> {
    build(view, host, cx, |registry| registry.pills.get(view), |build, cx| build(screen, cx))
}

pub(crate) fn build_panel(view: &SharedString, host: &str, screen: &Screen, cx: &mut App) -> Option<Panel> {
    build(view, host, cx, |registry| registry.panels.get(view), |build, cx| build(screen, cx))
}

pub(crate) fn build_overlay(view: &SharedString, host: &str, cx: &mut App) -> Option<Box<dyn Any>> {
    build(view, host, cx, |registry| registry.overlays.get(view), |build, cx| build(cx))
}

pub struct Parts {
    host: &'static str,
    screen: Screen,
    slots: Option<Entity<Slots>>,
    views: Kept<Part>,
    seen: Vec<bool>,
}

impl Parts {
    pub fn new(host: &'static str, screen: &Screen, cx: &mut App) -> Self {
        let mut parts =
            Self { host, screen: screen.clone(), slots: Stage::slots(cx), views: Kept::default(), seen: Vec::new() };
        parts.sync(cx);
        parts
    }

    pub fn sync(&mut self, cx: &mut App) -> bool {
        let wanted = self.slots.as_ref().map(|slots| slots.read(cx).for_host(self.host, cx)).unwrap_or_default();
        let (host, screen) = (self.host, &self.screen);
        let changed = self.views.sync(&wanted, |view| {
            build(view, host, cx, |registry| registry.parts.get(view), |build, cx| build(screen, cx))
        });
        self.shown_changed();
        changed
    }

    pub fn shown_changed(&mut self) -> bool {
        let now: Vec<bool> = self.views.iter().map(Part::is_shown).collect();
        let changed = now != self.seen;
        self.seen = now;
        changed
    }

    pub fn views(&self) -> impl Iterator<Item = &AnyView> {
        self.views.iter().map(Part::view)
    }

    pub fn parts(&self) -> impl Iterator<Item = &Part> {
        self.views.iter()
    }

    pub fn slots(&self) -> impl Iterator<Item = (&SharedString, Option<&Part>)> {
        self.views.entries()
    }

    pub fn get(&self, view: &str) -> Option<&Part> {
        self.slots().find(|(id, _)| id.as_ref() == view).and_then(|(_, part)| part)
    }

    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }
}
