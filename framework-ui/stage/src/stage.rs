use crate::kept::Kept;
use crate::registry;
use crate::rules::{OpenPanels, PanelRules};
use crate::screens::Screens;
use crate::slots::{HOST_SHELL_OVERLAYS, Slots};
use crate::view::{self, Shared, StageView};
use gpui::{
    App, AppContext, Context, DisplayId, Entity, Global, Pixels, SharedString, Size, Subscription, WindowHandle,
};
use nova_daemon_info::Slot;
use std::any::Any;
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct StageOptions {
    pub extra_slots: Vec<Slot>,
    pub log_frames: bool,
}

#[derive(Default)]
pub(crate) struct Panels {
    pub open: OpenPanels,
    known: HashMap<DisplayId, Vec<PanelRules>>,
    sizes: HashMap<(DisplayId, SharedString), Size<Pixels>>,
    places: HashMap<(DisplayId, SharedString), Pixels>,
    fullscreen: Vec<SharedString>,
    pages: HashMap<(DisplayId, SharedString), SharedString>,
    pub awake: bool,
}

impl Panels {
    fn rules(&self, name: &str, screen: DisplayId) -> Option<PanelRules> {
        let found = self.known.get(&screen).and_then(|panels| panels.iter().find(|panel| panel.name.as_ref() == name));
        if found.is_none() {
            eprintln!("WARN nova-ui: no panel {name} on this screen (its feature is off, or has no ui here yet)");
        }
        found.cloned()
    }

    pub fn know(&mut self, screen: DisplayId, panels: Vec<PanelRules>) -> bool {
        let closed = self.open.retain(|on, open| on != screen || panels.iter().any(|panel| panel.name == open.name));
        self.sizes.retain(|(on, name), _| *on != screen || panels.iter().any(|panel| panel.name == *name));
        self.pages.retain(|(on, name), _| *on != screen || panels.iter().any(|panel| panel.name == *name));
        self.places.retain(|(on, name), _| *on != screen || panels.iter().any(|panel| panel.name == *name));
        let names = |list: &[PanelRules]| list.iter().map(|panel| panel.name.clone()).collect::<Vec<_>>();
        let before = self.known.insert(screen, panels.clone());
        closed || before.is_none_or(|before| names(&before) != names(&panels))
    }

    fn has(&self, name: &str, screen: DisplayId) -> bool {
        self.known.get(&screen).is_some_and(|panels| panels.iter().any(|panel| panel.name.as_ref() == name))
    }

    fn keep_screens(&mut self, here: &[DisplayId]) -> bool {
        self.known.retain(|screen, _| here.contains(screen));
        self.sizes.retain(|(screen, _), _| here.contains(screen));
        self.pages.retain(|(screen, _), _| here.contains(screen));
        self.places.retain(|(screen, _), _| here.contains(screen));
        self.open.retain(|screen, _| here.contains(&screen))
    }

    pub fn size(&self, name: &SharedString, screen: DisplayId) -> Option<Size<Pixels>> {
        self.sizes.get(&(screen, name.clone())).copied()
    }

    pub fn place(&self, name: &SharedString, screen: DisplayId) -> Option<Pixels> {
        self.places.get(&(screen, name.clone())).copied()
    }

    pub fn fullscreen(&self, output: &str) -> bool {
        self.fullscreen.iter().any(|each| each.as_ref() == output)
    }
}

struct StageState {
    panels: Entity<Panels>,
    slots: Entity<Slots>,
    screens: Entity<Screens>,
    log_frames: bool,
    windows: Vec<(DisplayId, WindowHandle<StageView>)>,
    overlays: Kept<Box<dyn Any>>,
    _observers: Vec<Subscription>,
}

impl Global for StageState {}

pub struct Stage;

impl Stage {
    pub fn start(options: StageOptions, cx: &mut App) {
        let screens = Screens::follow(cx);
        let slots = Slots::follow(options.extra_slots, cx);
        let panels = cx.new(|_| Panels::default());
        let observers =
            vec![cx.observe(&screens, |_, cx| sync_windows(cx)), cx.observe(&slots, |_, cx| sync_overlays(cx))];
        cx.set_global(StageState {
            panels,
            slots,
            screens,
            log_frames: options.log_frames,
            windows: Vec::new(),
            overlays: Kept::default(),
            _observers: observers,
        });
        sync_windows(cx);
        sync_overlays(cx);
    }

    pub fn toggle(name: &str, screen: DisplayId, cx: &mut App) {
        change(cx, |panels| panels.rules(name, screen).is_some_and(|panel| panels.open.toggle(panel, screen)));
    }

    pub fn open(name: &str, screen: DisplayId, cx: &mut App) {
        change(cx, |panels| panels.rules(name, screen).is_some_and(|panel| panels.open.open(panel, screen)));
    }

    pub fn close(name: &str, screen: DisplayId, cx: &mut App) {
        change(cx, |panels| panels.open.close(name, screen));
    }

    pub fn toggle_on_output(name: &str, output: Option<&str>, cx: &mut App) {
        let screen = Self::screens(cx).and_then(|screens| screens.read(cx).for_output(output).map(|screen| screen.id));
        match screen {
            Some(screen) => Self::toggle(name, screen, cx),
            None => eprintln!("WARN nova-ui: no screen to toggle {name} on"),
        }
    }

    pub fn dismiss(cx: &mut App) {
        change(cx, |panels| panels.open.dismiss());
    }

    pub fn has_panel(name: &str, screen: DisplayId, cx: &App) -> bool {
        cx.try_global::<StageState>().is_some_and(|state| state.panels.read(cx).has(name, screen))
    }

    pub fn observe_panels<V: 'static>(
        cx: &mut Context<V>,
        mut changed: impl FnMut(&mut V, &mut Context<V>) + 'static,
    ) -> Option<Subscription> {
        let panels = cx.try_global::<StageState>()?.panels.clone();
        Some(cx.observe(&panels, move |view, _, cx| changed(view, cx)))
    }

    pub fn is_open(name: &str, screen: DisplayId, cx: &App) -> bool {
        cx.try_global::<StageState>().is_some_and(|state| state.panels.read(cx).open.is_open(name, screen))
    }

    pub fn resize(name: &str, screen: DisplayId, size: Size<Pixels>, cx: &mut App) {
        change(cx, |panels| {
            panels
                .rules(name, screen)
                .is_some_and(|panel| panels.sizes.insert((screen, panel.name), size) != Some(size))
        });
    }

    pub fn show_page(name: &str, screen: DisplayId, page: Option<&str>, cx: &mut App) {
        change(cx, |panels| {
            let key = (screen, SharedString::from(name.to_owned()));
            let before = match page {
                Some(page) => panels.pages.insert(key, SharedString::from(page.to_owned())),
                None => panels.pages.remove(&key),
            };
            before.as_deref() != page
        });
    }

    pub fn page(name: &str, screen: DisplayId, cx: &App) -> Option<SharedString> {
        let state = cx.try_global::<StageState>()?;
        state.panels.read(cx).pages.get(&(screen, SharedString::from(name.to_owned()))).cloned()
    }

    pub fn place(name: &str, screen: DisplayId, left: Pixels, cx: &mut App) {
        change(cx, |panels| {
            panels
                .rules(name, screen)
                .is_some_and(|panel| panels.places.insert((screen, panel.name), left) != Some(left))
        });
    }

    pub fn set_fullscreen(outputs: Vec<SharedString>, cx: &mut App) {
        change(cx, |panels| {
            let changed = panels.fullscreen != outputs;
            panels.fullscreen = outputs;
            changed
        });
    }

    pub fn keep_awake(awake: bool, cx: &mut App) {
        change(cx, |panels| {
            let changed = panels.awake != awake;
            panels.awake = awake;
            changed
        });
    }

    pub fn is_kept_awake(cx: &App) -> bool {
        cx.try_global::<StageState>().is_some_and(|state| state.panels.read(cx).awake)
    }

    pub fn observe_open(cx: &mut App, mut changed: impl FnMut(&mut App) + 'static) -> Option<Subscription> {
        let panels = cx.try_global::<StageState>().map(|state| state.panels.clone())?;
        Some(cx.observe(&panels, move |_, cx| changed(cx)))
    }

    pub fn logs_frames(cx: &App) -> bool {
        cx.try_global::<StageState>().is_some_and(|state| state.log_frames)
    }

    pub fn screens(cx: &App) -> Option<Entity<Screens>> {
        cx.try_global::<StageState>().map(|state| state.screens.clone())
    }

    pub fn slots(cx: &App) -> Option<Entity<Slots>> {
        cx.try_global::<StageState>().map(|state| state.slots.clone())
    }
}

fn change(cx: &mut App, change: impl FnOnce(&mut Panels) -> bool) {
    let Some(panels) = cx.try_global::<StageState>().map(|state| state.panels.clone()) else {
        eprintln!("WARN nova-ui: the stage was asked for a panel before it started");
        return;
    };
    panels.update(cx, |panels, cx| {
        if change(panels) {
            cx.notify();
        }
    });
}

fn sync_windows(cx: &mut App) {
    let Some(state) = cx.try_global::<StageState>() else {
        return;
    };
    let shared = Shared {
        panels: state.panels.clone(),
        slots: state.slots.clone(),
        screens: state.screens.clone(),
        log_frames: state.log_frames,
    };
    let screens = shared.screens.clone();
    let here = screens.read(cx).list().to_vec();
    let mut windows = std::mem::take(&mut cx.global_mut::<StageState>().windows);
    windows.retain(|(screen, window)| {
        if here.iter().any(|kept| kept.id == *screen) {
            return window.update(cx, |_, _, _| ()).is_ok();
        }
        eprintln!("INFO nova-ui: a screen left, closing its stage");
        let _ = window.update(cx, |stage, window, cx| {
            stage.leave(cx);
            window.remove_window();
        });
        false
    });
    for screen in &here {
        if windows.iter().any(|(open, _)| *open == screen.id) {
            continue;
        }
        eprintln!("INFO nova-ui: stage on screen {} ({:?})", screen.name, screen.size);
        if let Some(window) = view::open(screen, &shared, cx) {
            windows.push((screen.id, window));
        }
    }
    cx.global_mut::<StageState>().windows = windows;
    let ids: Vec<DisplayId> = here.iter().map(|screen| screen.id).collect();
    shared.panels.update(cx, |panels, cx| {
        if panels.keep_screens(&ids) {
            cx.notify();
        }
    });
}

fn sync_overlays(cx: &mut App) {
    let Some(state) = cx.try_global::<StageState>() else {
        return;
    };
    let wanted = state.slots.read(cx).for_host(HOST_SHELL_OVERLAYS, cx);
    let mut overlays = std::mem::take(&mut cx.global_mut::<StageState>().overlays);
    overlays.sync(&wanted, |view| registry::build_overlay(view, HOST_SHELL_OVERLAYS, cx));
    cx.global_mut::<StageState>().overlays = overlays;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn left() -> DisplayId {
        DisplayId::new(1)
    }

    fn right() -> DisplayId {
        DisplayId::new(2)
    }

    fn panel(name: &'static str) -> PanelRules {
        PanelRules {
            name: name.into(),
            placement: crate::Placement::Top,
            exclusive: true,
            wants_keyboard: false,
            closes_on_click_outside: true,
        }
    }

    #[test]
    fn a_panel_whose_feature_turned_off_closes() {
        let mut panels = Panels::default();
        assert!(panels.know(left(), vec![panel("launcher"), panel("power")]), "new panels are a change");
        assert!(!panels.know(left(), vec![panel("launcher"), panel("power")]), "the same ones are not");
        assert!(panels.has("launcher", left()));
        let launcher = panels.rules("launcher", left()).unwrap();
        panels.open.open(launcher, left());
        assert!(panels.know(left(), vec![panel("power")]));
        assert!(!panels.open.is_open("launcher", left()));
        assert_eq!(panels.rules("launcher", left()), None);
        assert!(!panels.has("launcher", left()));
    }

    #[test]
    fn an_unplugged_screen_closes_what_was_open_on_it() {
        let mut panels = Panels::default();
        panels.know(left(), vec![panel("launcher")]);
        panels.know(right(), vec![panel("launcher")]);
        let launcher = panels.rules("launcher", right()).unwrap();
        panels.open.open(launcher, right());
        assert!(panels.keep_screens(&[left()]));
        assert!(!panels.open.catches_clicks());
        assert_eq!(panels.rules("launcher", right()), None);
        assert!(panels.rules("launcher", left()).is_some());
    }
}
