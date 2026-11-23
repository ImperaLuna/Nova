//! The central pill (view `central-pill` in `bar.center`), ported from
//! `ui/central_pill/CentralPill.qml` and `CentralPillSlot.qml`.
//!
//! Left to right: the search button while the launcher is ready, then the
//! parts of `central-pill.parts` in slot order (the workspace numbers, the
//! day and the time), a divider between each two. The pill's ends sit as
//! far from the first and last thing as that thing asks ([`nova_stage::Part::inset`]):
//! round cells closer than text. The clock part keeps its time at the
//! screen's middle (`nova_stage::middle`), so the pill grows to the left as
//! workspaces come and go; without the clock the pill is centered. With
//! nothing to show the stage leaves it out.
//!
//! Scrolling anywhere over it steps through the screen's workspaces, one
//! per wheel notch, while the compositor can (capability
//! `focus_workspace`). The button only toggles the launcher, which fades in
//! where it stands, not out of this pill.

use gpui::{
    AnyElement, App, AppContext, Context, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Render, ScrollEvent, StatefulInteractiveElement, Styled, Subscription, Window, div,
};
use nova_bar_api::Panel as BarPanel;
use nova_desktop_info::{Capability, Desktop, Event as DesktopEvent, Step};
use nova_look::motion::Eased;
use nova_look::tokens::{
    DIVIDER_HEIGHT, DIVIDER_WIDTH, DURATION_FAST, GAP, ICON_SIZE_SMALL, WORKSPACE_INSET, WORKSPACE_SIZE,
};
use nova_look::{Glyph, Icon, Theme, Wheel, mix};
use nova_stage::{Parts, Pill, Screen, Shown, Stage};
use nova_ui_client::{Daemon, Live};
use nova_workspaces_api::Command as WorkspacesCommand;
use std::time::Instant;

/// Its view id, which the launcher's panel names to grow out of it.
const VIEW: &str = "central-pill";
/// The host its parts come from.
const PARTS: &str = "central-pill.parts";

pub(crate) fn register(cx: &mut App) {
    nova_stage::pill(VIEW, cx, |screen, cx| {
        let shown = Shown::new(false);
        let pill = cx.new(|cx| CentralPill::new(screen, shown.clone(), cx));
        // Its own buttons decide what a click does.
        Pill::new(pill).opens_on_click(false).shown_while(shown)
    });
}

struct CentralPill {
    screen: Screen,
    parts: Parts,
    desktop: Entity<Live<Desktop>>,
    /// Whether it has something to show, for the stage.
    shown: Shown,
    /// The search button's hover wash: 0 none, 1 full.
    search_hover: Eased,
    wheel: Wheel,
    _observers: Vec<Subscription>,
}

impl CentralPill {
    fn new(screen: &Screen, shown: Shown, cx: &mut Context<Self>) -> Self {
        let desktop = Live::follow(nova_desktop_info::TOPIC, cx, |DesktopEvent::Compositor(desktop)| Some(desktop));
        // The launcher's panel comes and goes with its feature.
        let mut observers: Vec<Subscription> =
            Stage::observe_panels(cx, |pill, cx| pill.changed(cx)).into_iter().collect();
        if let Some(slots) = Stage::slots(cx) {
            observers.push(cx.observe(&slots, |pill, _, cx| {
                pill.parts.sync(cx);
                pill.changed(cx);
            }));
        }
        // A part came or went (`Part::shown_while`).
        observers.push(Shown::observe(cx, |pill, cx| {
            if pill.parts.shown_changed() {
                cx.notify();
            }
        }));
        let pill = Self {
            screen: screen.clone(),
            parts: Parts::new(PARTS, screen, cx),
            desktop,
            shown,
            search_hover: Eased::lasting(0.0, DURATION_FAST, Instant::now()),
            wheel: Wheel::default(),
            _observers: observers,
        };
        pill.shown.set(pill.has_content(cx), cx);
        pill
    }

    /// The launcher's panel is on this screen's stage (its feature is
    /// ready and its ui registered it), so the search button shows.
    fn can_search(&self, cx: &App) -> bool {
        Stage::has_panel(BarPanel::Launcher.name(), self.screen.id, cx)
    }

    fn can_step(&self, cx: &App) -> bool {
        let desktop = self.desktop.read(cx).get();
        desktop.is_some_and(|desktop| desktop.capabilities.contains(&Capability::FocusWorkspace))
    }

    fn has_content(&self, cx: &App) -> bool {
        self.can_search(cx) || !self.parts.is_empty()
    }

    /// The panels or the parts changed.
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.shown.set(self.has_content(cx), cx);
        cx.notify();
    }

    fn scrolled(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let notches = self.wheel.notches(event);
        if notches == 0 || self.screen.name.is_empty() || !self.can_step(cx) {
            return;
        }
        // Down (below zero) goes to the next workspace, as today.
        let step = if notches < 0 { Step::Next } else { Step::Previous };
        for _ in 0..notches.unsigned_abs() {
            let output = self.screen.name.to_string();
            Daemon::send(WorkspacesCommand::StepWorkspace { output, step }, cx);
        }
    }

    fn hover_search(&mut self, hovered: bool, cx: &mut Context<Self>) {
        let target = if hovered { 1.0 } else { 0.0 };
        if self.search_hover.target() != target {
            self.search_hover.set_target(target, Instant::now());
            cx.notify();
        }
    }

    /// The round search button, washed while the pointer is on it.
    fn search(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let now = Instant::now();
        if !self.search_hover.settled(now) {
            window.request_animation_frame();
        }
        let hover = self.search_hover.value(now);
        let theme = Theme::of(cx);
        let (wash, icon) = (theme.hover().opacity(hover), mix(theme.fg_dim(), theme.fg, hover));
        let screen = self.screen.id;
        div()
            .id("search")
            .size(WORKSPACE_SIZE)
            .flex_none()
            .rounded_full()
            .bg(wash)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(Icon::new(Glyph::SEARCH).size(ICON_SIZE_SMALL).color(icon))
            .on_hover(cx.listener(|pill, hovered: &bool, _, cx| pill.hover_search(*hovered, cx)))
            .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, _, cx| {
                Stage::toggle(BarPanel::Launcher.name(), screen, cx);
            })
            .into_any_element()
    }
}

impl Render for CentralPill {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // What it shows, each with how far it keeps from the pill's end.
        let mut shown: Vec<(AnyElement, gpui::Pixels)> = Vec::new();
        if self.can_search(cx) {
            shown.push((self.search(window, cx), WORKSPACE_INSET));
        }
        for part in self.parts.parts() {
            let content = div().h_full().flex().flex_none().items_center().child(part.view().clone());
            shown.push((content.into_any_element(), part.end_inset()));
        }
        let (Some(&(_, start)), Some(&(_, end))) = (shown.first(), shown.last()) else {
            return div();
        };
        let rim = Theme::of(cx).glass_rim();
        let mut children = Vec::with_capacity(shown.len() * 2);
        for (at, (element, _)) in shown.into_iter().enumerate() {
            if at > 0 {
                children.push(div().w(DIVIDER_WIDTH).h(DIVIDER_HEIGHT).flex_none().bg(rim).into_any_element());
            }
            children.push(element);
        }
        div()
            .h_full()
            .flex()
            .items_center()
            .gap(GAP)
            .pl(start)
            .pr(end)
            .on_scroll(cx.listener(Self::scrolled))
            .children(children)
    }
}
