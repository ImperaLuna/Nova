//! The `workspaces.list` view on one screen: the numbers of its output, as
//! [`crate::numbers`] places them, the capsule behind the shown one, and a
//! click on a number going there.

use crate::numbers::{Fade, Ink, Number, Numbers, shown};
use gpui::{
    AnyElement, App, Context, Entity, Hsla, InteractiveElement, IntoElement, ParentElement, Pixels, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use nova_desktop_info::{Capability, Desktop, Event, TOPIC, WorkspaceId};
use nova_look::tokens::{
    BAR_HEIGHT, DOT_SIZE, FONT_MONO, FONT_SIZE_SMALL, ICON_SIZE_SMALL, RING_INSET, WEIGHT_LABEL, WORKSPACE_INSET,
    WORKSPACE_SIZE,
};
use nova_look::{Glyph, Icon, Theme, mix};
use nova_stage::Screen;
use nova_ui_client::{Daemon, Live};
use nova_workspaces_api::Command;
use std::time::Instant;

pub struct WorkspaceList {
    screen: Screen,
    desktop: Entity<Live<Desktop>>,
    numbers: Numbers,
    /// Going to a workspace works (capability `focus_workspace`).
    can_focus: bool,
}

impl WorkspaceList {
    pub fn new(screen: Screen, cx: &mut Context<Self>) -> Self {
        let desktop = Live::follow(TOPIC, cx, |Event::Compositor(desktop)| Some(desktop));
        cx.observe(&desktop, |list, _, cx| list.follow(cx)).detach();
        let mut list = Self { screen, desktop, numbers: Numbers::new(Instant::now()), can_focus: false };
        list.follow(cx);
        list
    }

    /// Takes in what the compositor shows now.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let desktop = self.desktop.read(cx).get();
        let workspaces = desktop.map(|desktop| shown(desktop, &self.screen.name)).unwrap_or_default();
        let output_focused = desktop
            .and_then(|desktop| desktop.outputs.iter().find(|output| output.name == self.screen.name.as_ref()))
            .is_some_and(|output| output.focused);
        self.can_focus = desktop.is_some_and(|desktop| desktop.capabilities.contains(&Capability::FocusWorkspace));
        self.numbers.set(workspaces, output_focused, Instant::now());
        cx.notify();
    }
}

fn color(ink: Ink, theme: &Theme) -> Hsla {
    match ink {
        Ink::OnAccent => theme.on_accent,
        Ink::Calling => theme.primary,
        Ink::Dim => theme.fg_dim(),
        Ink::Plain => theme.fg,
    }
}

fn faded<T: Copy + PartialEq>(fade: &Fade<T>, now: Instant, color: impl Fn(T) -> Hsla) -> Hsla {
    let (from, to, progress) = fade.at(now);
    mix(color(from), color(to), progress)
}

/// What a number says: a tilde for the only special workspace, a special
/// one's first letter, or its name.
fn label(number: &Number, lone_special: bool, ink: Hsla, size: Pixels) -> AnyElement {
    let workspace = &number.workspace;
    if workspace.special && lone_special {
        return Icon::new(Glyph::TILDE)
            .size(ICON_SIZE_SMALL * (f32::from(size) / f32::from(WORKSPACE_SIZE)))
            .color(ink)
            .into_any_element();
    }
    let text: String = if workspace.special {
        workspace.name.chars().next().map(|first| first.to_uppercase().collect()).unwrap_or_default()
    } else {
        workspace.name.clone()
    };
    div()
        .w(size - RING_INSET * 2.0)
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_center()
        .text_color(ink)
        .child(SharedString::from(text))
        .into_any_element()
}

impl WorkspaceList {
    fn number(&self, number: &Number, now: Instant, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let ink = faded(&number.ink, now, |ink| color(ink, theme));
        // A new number grows in from smaller, around its middle.
        let scale = number.scale.value(now);
        let size = WORKSPACE_SIZE * scale;
        let on_capsule = number.workspace.active && self.numbers.output_focused();
        let hover = theme.hover();
        let mut cell = div()
            .id(SharedString::from(format!("workspace{}", number.id())))
            .size(size)
            .rounded(size / 2.0)
            .flex()
            .items_center()
            .justify_center()
            .text_size(FONT_SIZE_SMALL * scale)
            .child(label(number, self.numbers.lone_special(), ink, size));
        if !on_capsule && self.can_focus {
            cell = cell.hover(move |style| style.bg(hover));
        }
        if self.can_focus {
            let id: WorkspaceId = number.id();
            cell = cell
                .cursor_pointer()
                .on_click(move |_, _, cx: &mut App| Daemon::send(Command::FocusWorkspace { id }, cx));
        }
        let dot_opacity = number.dot.value(now);
        let dot = (dot_opacity > 0.0).then(|| {
            div()
                .absolute()
                .left((WORKSPACE_SIZE - DOT_SIZE) / 2.0)
                .top((BAR_HEIGHT + WORKSPACE_SIZE) / 2.0 - RING_INSET - DOT_SIZE)
                .size(DOT_SIZE)
                .rounded(DOT_SIZE / 2.0)
                .bg(theme.primary)
                .opacity(dot_opacity)
        });
        div()
            .absolute()
            .left(px(number.x.value(now)))
            .top_0()
            .w(WORKSPACE_SIZE)
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .opacity(number.opacity.value(now))
            .child(cell)
            .children(dot)
            .into_any_element()
    }
}

impl Render for WorkspaceList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        if self.numbers.moving(now) {
            window.request_animation_frame();
        }
        let theme = Theme::of(cx);
        let capsule = self.numbers.capsule_x(now).map(|x| {
            let fill = faded_accent(&self.numbers, now, theme);
            div()
                .absolute()
                .left(px(x))
                .top(WORKSPACE_INSET)
                .size(WORKSPACE_SIZE)
                .rounded(WORKSPACE_SIZE / 2.0)
                .bg(fill)
        });
        let numbers: Vec<AnyElement> = self.numbers.iter().map(|number| self.number(number, now, cx)).collect();
        div()
            .relative()
            .h(BAR_HEIGHT)
            .w(px(self.numbers.width(now)))
            .font_family(FONT_MONO)
            .font_weight(WEIGHT_LABEL)
            .children(capsule)
            .children(numbers)
    }
}

/// The capsule's fill: the accent while the output has the focus, a quiet
/// fill otherwise, fading between the two.
fn faded_accent(numbers: &Numbers, now: Instant, theme: &Theme) -> Hsla {
    let (from, to, progress) = numbers.capsule_accent(now);
    let fill = |accent: bool| if accent { theme.primary } else { theme.selected_quiet() };
    mix(fill(from), fill(to), progress)
}
