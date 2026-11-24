use crate::choice::{ENTRIES, Entry};
use crate::menu::{Menu, problem_width};
use crate::tokens::{BUSY_OPACITY, ROW_GAP};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, Hsla, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement,
    Styled, Window, div, relative, transparent_black,
};
use nova_look::tokens::{
    DRAIN_HEIGHT, DRAIN_OPACITY, DURATION_FAST, FONT_MONO, FONT_SIZE, FONT_SIZE_SMALL, ICON_SIZE_MENU, LABEL_NUDGE,
    LIST_ROW_HEIGHT, PAD, RADIUS_ROW,
};
use nova_look::{Glyph, Icon, Spinner, Theme, fade_color};
use nova_power_api::PowerAction;
use std::time::Instant;

fn glyph(action: PowerAction) -> Glyph {
    match action {
        PowerAction::Lock => Glyph::LOCK,
        PowerAction::LogOut => Glyph::LOG_OUT,
        PowerAction::Suspend => Glyph::MOON,
        PowerAction::Reboot => Glyph::REBOOT,
        PowerAction::PowerOff => Glyph::POWER,
    }
}

struct RowState {
    selected: bool,
    available: bool,
    armed: Option<(f32, u64)>,
    running: bool,
    busy: bool,
    reason: String,
}

impl Menu {
    fn row_state(&self, action: PowerAction, now: Instant) -> RowState {
        let choice = &self.choice;
        let armed = (choice.armed(now) == Some(action)).then(|| (choice.armed_left(now), choice.seconds_left(now)));
        let running = self.open && choice.running == Some(action);
        RowState {
            selected: choice.selected == action,
            available: choice.available(action),
            armed,
            running,
            busy: choice.running.is_some() && !running,
            reason: choice.reason(action).to_owned(),
        }
    }

    fn header(&self, cx: &Context<Self>) -> impl IntoElement {
        let dim = Theme::of(cx).fg_dim();
        div()
            .flex()
            .items_baseline()
            .justify_between()
            .py(LABEL_NUDGE)
            .text_size(FONT_SIZE_SMALL)
            .text_color(dim)
            .child(div().pl(PAD + LABEL_NUDGE).child("SESSION"))
            .children(self.uptime().map(|uptime| div().pr(PAD).font_family(FONT_MONO).child(uptime)))
    }

    fn row(&self, entry: &Entry, now: Instant, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let action = entry.action;
        let state = self.row_state(action, now);
        let theme = Theme::of(cx);
        let ink = if state.selected { theme.on_accent } else { theme.fg };
        let reason_ink = if state.selected { theme.on_accent.alpha(DRAIN_OPACITY) } else { theme.fg_dim() };
        let fill = if state.selected { theme.accent_fill() } else { transparent_black() };
        let fill = fade_color(("power-row-fill", action as usize), fill, DURATION_FAST, window, cx);
        let label = match (state.running, state.armed) {
            (true, _) => format!("{}…", entry.doing),
            (false, Some(_)) => format!("{}?", entry.label),
            (false, None) => entry.label.to_owned(),
        };
        let reason = (state.armed.is_none() && !state.available && !state.reason.is_empty())
            .then(|| div().text_size(FONT_SIZE_SMALL).text_color(reason_ink).truncate().child(state.reason.clone()));
        let enabled = state.available && !state.busy;
        div()
            .id(("power-row", action as usize))
            .relative()
            .flex_none()
            .h(LIST_ROW_HEIGHT)
            .rounded(RADIUS_ROW)
            .bg(fill)
            .flex()
            .items_center()
            .gap(PAD)
            .pl(PAD + LABEL_NUDGE)
            .pr(PAD)
            .when(!enabled, |row| row.opacity(BUSY_OPACITY))
            .when(enabled, |row| {
                row.on_hover(cx.listener(move |menu, hovered: &bool, _, cx| {
                    if *hovered {
                        menu.hover(action, cx);
                    }
                }))
                .on_click(cx.listener(move |menu, _, _, cx| menu.pick(action, cx)))
            })
            .children(state.armed.map(|(left, _)| drain(left, ink)))
            .child(Icon::new(glyph(action)).size(ICON_SIZE_MENU).color(ink))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .text_color(ink)
                    .child(div().text_size(FONT_SIZE).truncate().child(label))
                    .children(reason),
            )
            .children(end(&state, ink))
            .into_any_element()
    }
}

fn drain(left: f32, ink: Hsla) -> impl IntoElement {
    div().absolute().left_0().bottom_0().w(relative(left)).h(DRAIN_HEIGHT).bg(ink.alpha(DRAIN_OPACITY))
}

fn end(state: &RowState, ink: Hsla) -> Option<AnyElement> {
    if state.running {
        return Some(Spinner::new().color(ink).into_any_element());
    }
    if !state.selected {
        return None;
    }
    let text = match state.armed {
        Some((_, seconds)) => format!("{seconds} s"),
        None => "↵".to_owned(),
    };
    Some(div().flex_none().font_family(FONT_MONO).text_size(FONT_SIZE_SMALL).child(text).into_any_element())
}

impl Render for Menu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let spinning = self.open && self.choice.running.is_some();
        if self.choice.armed(now).is_some() || spinning {
            window.request_animation_frame();
        }
        let rows: Vec<AnyElement> = ENTRIES.iter().map(|entry| self.row(entry, now, window, cx)).collect();
        let error = Theme::of(cx).error;
        let problem = (!self.choice.problem.is_empty()).then(|| {
            div()
                .w(problem_width() + PAD + LABEL_NUDGE + PAD)
                .pl(PAD + LABEL_NUDGE)
                .pr(PAD)
                .text_size(FONT_SIZE_SMALL)
                .text_color(error)
                .child(self.choice.problem.clone())
        });
        div()
            .id("power-menu")
            .size_full()
            .track_focus(&self.focus())
            .on_key_down(cx.listener(Menu::key))
            .p(PAD)
            .flex()
            .flex_col()
            .justify_center()
            .gap(ROW_GAP)
            .child(self.header(cx))
            .children(rows)
            .children(problem)
    }
}
