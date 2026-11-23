//! Where each feature's ui says what its views are. A manifest's `[[slot]]`
//! names a view by id; the feature's `register` gives the stage a function
//! that builds it. Nothing is built until a slot of a ready feature asks
//! for the view, so a feature that is off costs one map entry.

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

/// Registers the pill a `bar.left`, `bar.center` or `bar.right` slot names
/// as `view`. `build` runs once per screen the pill shows on.
pub fn pill(view: &'static str, cx: &mut App, build: impl Fn(&Screen, &mut App) -> Pill + 'static) {
    cx.default_global::<Registry>().pills.insert(view.into(), Rc::new(build));
}

/// Registers the panel a `stage.panels` slot names as `view`: one that is
/// no pill's own.
pub fn panel(view: &'static str, cx: &mut App, build: impl Fn(&Screen, &mut App) -> Panel + 'static) {
    cx.default_global::<Registry>().panels.insert(view.into(), Rc::new(build));
}

/// Registers a view for a host some feature draws itself (the central
/// pill's parts, the right pill's buttons, quick settings' tiles). That
/// feature shows its host's views with [`Parts`]. `build` returns a
/// [`Part`]: `cx.new(View::new).into()`, or [`Part::new`] with its flags.
pub fn part(view: &'static str, cx: &mut App, build: impl Fn(&Screen, &mut App) -> Part + 'static) {
    cx.default_global::<Registry>().parts.insert(view.into(), Rc::new(build));
}

/// Registers what a `shell.overlays` slot names as `view`: something with
/// windows of its own over the screens (pop-ups, the volume capsule).
/// `build` runs once while its feature is ready and returns whatever keeps
/// it alive (an entity that owns its windows and follows
/// [`crate::Stage::screens`]); the stage drops that when the feature goes.
pub fn overlay(view: &'static str, cx: &mut App, build: impl Fn(&mut App) -> Box<dyn Any> + 'static) {
    cx.default_global::<Registry>().overlays.insert(view.into(), Rc::new(build));
}

/// Builds what `find` picks from the registry, or warns that `host` has a
/// slot for a view nothing registered.
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

/// The views of a host a feature draws itself, on one screen: kept in step
/// with the host's slots, each view built once and kept while its slot
/// stays.
///
/// The feature that owns the host keeps one per screen, calls
/// [`Parts::sync`] when [`crate::Stage::slots`] notifies, and draws
/// [`Parts::views`] in order.
pub struct Parts {
    host: &'static str,
    screen: Screen,
    /// `None` before the stage started: there are no slots to fill yet.
    slots: Option<Entity<Slots>>,
    views: Kept<Part>,
}

impl Parts {
    /// The parts of `host` on `screen`, built right away.
    pub fn new(host: &'static str, screen: &Screen, cx: &mut App) -> Self {
        let mut parts = Self { host, screen: screen.clone(), slots: Stage::slots(cx), views: Kept::default() };
        parts.sync(cx);
        parts
    }

    /// Follows the host's slots as they are now. Returns whether the views
    /// changed, so the caller redraws.
    pub fn sync(&mut self, cx: &mut App) -> bool {
        let wanted = self.slots.as_ref().map(|slots| slots.read(cx).for_host(self.host, cx)).unwrap_or_default();
        let (host, screen) = (self.host, &self.screen);
        self.views.sync(&wanted, |view| {
            build(view, host, cx, |registry| registry.parts.get(view), |build, cx| build(screen, cx))
        })
    }

    pub fn views(&self) -> impl Iterator<Item = &AnyView> {
        self.views.iter().map(Part::view)
    }

    /// The parts with their flags, in order.
    pub fn parts(&self) -> impl Iterator<Item = &Part> {
        self.views.iter()
    }

    /// Every slot of the host in order, by its view id, with its part:
    /// `None` for a view no feature's ui registered yet, which a host may
    /// still keep room for (quick settings' empty tiles).
    pub fn slots(&self) -> impl Iterator<Item = (&SharedString, Option<&Part>)> {
        self.views.entries()
    }

    /// The part whose view id is `view`, when it was built.
    pub fn get(&self, view: &str) -> Option<&Part> {
        self.slots().find(|(id, _)| id.as_ref() == view).and_then(|(_, part)| part)
    }

    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }
}
