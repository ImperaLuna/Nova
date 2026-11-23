//! Quick settings' notifications (`notifications.list`): a heading with how
//! many there are, Clear all, the do not disturb bell and the settings
//! gear; then the notifications grouped by app, the app with the newest
//! first. An app with two or more is a stack (its newest, with the edges of
//! the rest under it and "+N"), which a click opens in place, with Show
//! less and Clear. With none, "All caught up".
//! `ui/quick_settings/NotificationList.qml` and `ui/notifications/
//! NotificationRow.qml` in today's shell.
//!
//! Quick settings draws it (port 17 onwards); until then `nova-ui --demo`
//! shows it in a panel of its own.

use crate::logic::{Group, ago, groups};
use crate::model::{self, Model};
use crate::pieces::{RoundButton, battery_amount, who};
use crate::tokens::{
    BUTTON, CHIP_HEIGHT, CHIP_PAD, CLOSE_INSET, CLOSE_SIZE, EMPTY_ICON, LIST_GAP, MENU_ITEM_HEIGHT, MENU_PAD,
    MENU_WIDTH, PEEK, PEEK_HEIGHT, PEEK_INSET, ROW_ICON, ROW_LINES,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, StatefulInteractiveElement, Styled, Window, deferred, div,
};
use nova_look::tokens::{
    DIVIDER_WIDTH, DOT_SIZE, FONT_DISPLAY, FONT_MONO, FONT_SIZE_DISPLAY, FONT_SIZE_MICRO, FONT_SIZE_SMALL, GAP,
    ICON_GAP, ICON_SIZE_SMALL, LABEL_NUDGE, PAD, RADIUS_CARD, RADIUS_CONTROL, RADIUS_SEGMENT, ROW_INSET, WEIGHT_LABEL,
    WEIGHT_TITLE,
};
use nova_look::{Glyph, Icon, LookStyled, Theme};
use nova_notifications_api::Notification;
use nova_stage::{Screen, Stage};
use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Quick settings' panel, which opening a notification closes.
const QUICK_SETTINGS: &str = "quick-settings";
/// "5m" moves on.
const CLOCK_TICK: Duration = Duration::from_secs(60);
/// How many edges a closed stack shows under its top one at most.
const PEEKS: usize = 2;

pub struct List {
    screen: Screen,
    model: Entity<Model>,
    /// The apps whose stacks are open.
    expanded: HashSet<String>,
    /// The notification whose ⋯ menu is open, and the one under the pointer.
    menu_for: Option<u32>,
    hovered: Option<u32>,
}

impl List {
    pub fn new(screen: Screen, cx: &mut Context<Self>) -> Self {
        let model = Model::get(cx);
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        cx.spawn(async move |list, cx| {
            let mut shown = true;
            while shown {
                cx.background_executor().timer(CLOCK_TICK).await;
                shown = list.update(cx, |_, cx| cx.notify()).is_ok();
            }
        })
        .detach();
        Self { screen, model, expanded: HashSet::new(), menu_for: None, hovered: None }
    }

    fn heading(&self, count: usize, do_not_disturb: bool, cx: &Context<Self>) -> Div {
        let theme = Theme::of(cx);
        let title =
            div().flex().items_center().gap(GAP).child(card_title("Notifications", cx)).when(count > 0, |title| {
                title.child(
                    div()
                        .h(CHIP_HEIGHT)
                        .px(CHIP_PAD)
                        .flex()
                        .items_center()
                        .rounded_full()
                        .bg(theme.primary)
                        .font_family(FONT_MONO)
                        .text_size(FONT_SIZE_MICRO)
                        .text_color(theme.on_accent)
                        .child(count.to_string()),
                )
            });
        let bell = if do_not_disturb { Glyph::BELL_OFF } else { Glyph::BELL };
        let screen = self.screen.id;
        let buttons = div()
            .flex()
            .items_center()
            .gap(ICON_GAP)
            .when(count > 0, |buttons| {
                buttons.child(text_button("clear-all", "Clear all", cx).on_click(|_, _, cx| model::clear(None, cx)))
            })
            .child(
                RoundButton::new("do-not-disturb", bell, BUTTON)
                    .on(do_not_disturb)
                    .build(cx, move |_, _, cx| model::set_do_not_disturb(!do_not_disturb, cx)),
            )
            .child(RoundButton::new("settings", Glyph::GEAR, BUTTON).build(cx, move |_, _, cx| {
                Stage::show_page(QUICK_SETTINGS, screen, Some(crate::PAGE), cx);
            }));
        div().h(BUTTON).flex().items_center().justify_between().child(title).child(buttons)
    }

    fn empty(cx: &App) -> Div {
        let theme = Theme::of(cx);
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(GAP)
            .child(Icon::new(Glyph::BELL).size(EMPTY_ICON).color(theme.fg_dim()))
            .child(div().font_family(FONT_DISPLAY).italic().text_size(FONT_SIZE_DISPLAY).child("All caught up"))
    }

    fn group(&self, group: &Group<'_>, now: i64, cx: &Context<Self>) -> AnyElement {
        let [newest, rest @ ..] = group.items.as_slice() else { return div().into_any_element() };
        if rest.is_empty() {
            return self.row(newest, Shown { app: true, stack_of: None }, now, cx).into_any_element();
        }
        if self.expanded.contains(group.app) {
            return self.open_stack(group, now, cx).into_any_element();
        }
        let theme = Theme::of(cx);
        let peeks = rest.len().min(PEEKS);
        // Each edge shows under the one above it: the nearest a deeper tone.
        let edges = (1..=peeks).rev().map(|level| {
            let tone = if level == 1 { theme.selected() } else { theme.surface_raised() };
            div()
                .absolute()
                .left(PEEK_INSET * level as f32)
                .right(PEEK_INSET * level as f32)
                .bottom(PEEK * (peeks - level) as f32)
                .h(PEEK_HEIGHT)
                .rounded_full()
                .bg(tone)
        });
        let count = div()
            .absolute()
            .right(PAD)
            .bottom_0()
            .h(CHIP_HEIGHT)
            .px(CHIP_PAD)
            .flex()
            .items_center()
            .rounded_full()
            .bg(theme.bg)
            .border(DIVIDER_WIDTH)
            .border_color(theme.glass_rim())
            .font_family(FONT_MONO)
            .text_size(FONT_SIZE_MICRO)
            .child(format!("+{}", rest.len()));
        let top = self.row(newest, Shown { app: true, stack_of: Some(group.app.to_owned()) }, now, cx);
        div().relative().pb(PEEK * peeks as f32).children(edges).child(top).child(count).into_any_element()
    }

    fn open_stack(&self, group: &Group<'_>, now: i64, cx: &Context<Self>) -> Div {
        let theme = Theme::of(cx);
        let app = group.app.to_owned();
        let (less, clear) = (app.clone(), app.clone());
        let heading = div()
            .h(CHIP_HEIGHT + LABEL_NUDGE * 4.0)
            .pl(LABEL_NUDGE * 4.0)
            .flex()
            .items_center()
            .gap(GAP)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(FONT_SIZE_SMALL)
                    .font_weight(WEIGHT_TITLE)
                    .child(group.app.to_owned()),
            )
            .child(
                div()
                    .font_family(FONT_MONO)
                    .text_size(FONT_SIZE_SMALL)
                    .text_color(theme.fg_dim())
                    .child(group.items.len().to_string()),
            )
            .child(div().flex_1())
            .child(text_button(("show-less", hash(&app)), "Show less", cx).on_click(cx.listener(
                move |list, _, _, cx| {
                    list.expanded.remove(&less);
                    cx.notify();
                },
            )))
            .child(text_button(("clear-app", hash(&app)), "Clear", cx).on_click(cx.listener(move |list, _, _, cx| {
                list.expanded.remove(&clear);
                model::clear(Some(&clear), cx);
                cx.notify();
            })));
        let rows: Vec<Div> =
            group.items.iter().map(|item| self.row(item, Shown { app: false, stack_of: None }, now, cx)).collect();
        div().flex().flex_col().gap(LIST_GAP).child(heading).children(rows)
    }

    /// A notification's row: its icon, "App · summary", how long ago, and
    /// the body in two lines; a dot while it's unread. A low battery alert
    /// is one line: the device's name, then its battery and percent. A click
    /// opens it (or its app's stack); ⋯ on hover offers muting its app and
    /// dismissing it.
    fn row(&self, notification: &Notification, shown: Shown, now: i64, cx: &Context<Self>) -> Div {
        let theme = Theme::of(cx);
        let id = notification.id;
        let hovered = self.hovered == Some(id);
        let menu_open = self.menu_for == Some(id);
        let battery = notification.device.map(|device| device.battery);
        let lines = Self::row_lines(notification, &shown, hovered || menu_open, now, cx);

        let clicked = notification.clone();
        let stack_of = shown.stack_of;
        let output = self.screen.name.clone();
        let screen = self.screen.id;
        let row = div()
            .id(("row", u64::from(id)))
            .relative()
            .px(PAD)
            .py(ROW_INSET)
            .flex()
            .gap(ROW_INSET)
            .when(battery.is_some(), |row| row.items_center())
            .rounded(RADIUS_CARD)
            .bg(if hovered { theme.hover() } else { theme.surface_raised() })
            .cursor_pointer()
            .on_hover(cx.listener(move |list, hovering: &bool, _, cx| {
                list.hovered = if *hovering { Some(id) } else { list.hovered.filter(|was| *was != id) };
                cx.notify();
            }))
            .on_click(cx.listener(move |list, _, _, cx| {
                list.menu_for = None;
                match &stack_of {
                    Some(app) => {
                        list.expanded.insert(app.clone());
                    }
                    None => {
                        model::open(&clicked, &output, cx);
                        Stage::close(QUICK_SETTINGS, screen, cx);
                    }
                }
                cx.notify();
            }))
            .when(notification.unread, |row| {
                row.child(
                    div()
                        .absolute()
                        .left((PAD - DOT_SIZE) / 2.0)
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(div().size(DOT_SIZE).rounded_full().bg(theme.primary)),
                )
            })
            .child(div().flex_none().child(who(notification, ROW_ICON, cx)))
            .child(lines);
        let more = (hovered || menu_open).then(|| {
            let toggle = cx.listener(move |list: &mut Self, _: &ClickEvent, _, cx| {
                list.menu_for = if list.menu_for == Some(id) { None } else { Some(id) };
                cx.notify();
            });
            div().absolute().top(CLOSE_INSET).right(CLOSE_INSET).child(
                RoundButton::new(("more", u64::from(id)), Glyph::DOTS, CLOSE_SIZE).filled().quiet().build(cx, toggle),
            )
        });
        let menu = menu_open.then(|| self.menu(notification, cx));
        div().child(row.children(more).children(menu))
    }

    /// "App · summary" (or the summary alone), how long ago or room for ⋯,
    /// and the body in two lines; a battery and percent instead for a low
    /// battery alert.
    fn row_lines(notification: &Notification, shown: &Shown, pointed: bool, now: i64, cx: &App) -> Div {
        let theme = Theme::of(cx);
        let battery = notification.device.map(|device| device.battery);
        let title = if notification.summary.is_empty() {
            notification.app.clone()
        } else if shown.app {
            format!("{} · {}", notification.app, notification.summary)
        } else {
            notification.summary.clone()
        };
        let end = div()
            .flex()
            .items_center()
            .gap(GAP)
            .children(battery.map(|battery| battery_amount(notification, battery, cx)))
            .map(|end| {
                if pointed {
                    // ⋯ takes the time's place.
                    end.child(div().w(CLOSE_SIZE))
                } else {
                    end.child(
                        div()
                            .font_family(FONT_MONO)
                            .text_size(FONT_SIZE_SMALL)
                            .text_color(theme.fg_soft())
                            .child(ago(notification.time, now)),
                    )
                }
            });
        let title_line = div()
            .flex()
            .items_center()
            .gap(GAP)
            .child(
                div().flex_1().min_w_0().truncate().text_size(FONT_SIZE_SMALL).font_weight(WEIGHT_TITLE).child(title),
            )
            .child(end);
        let body = (battery.is_none() && !notification.body.is_empty()).then(|| {
            div()
                .text_size(FONT_SIZE_SMALL)
                .text_color(theme.fg_dim())
                .line_clamp(ROW_LINES)
                .text_ellipsis()
                .child(notification.body.clone())
        });
        div().flex_1().min_w_0().flex().flex_col().child(title_line).children(body)
    }

    /// The ⋯ menu: mute its app, dismiss it. Drawn over the rows below.
    fn menu(&self, notification: &Notification, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let (id, app) = (notification.id, notification.app.clone());
        let item = |key: &'static str, glyph: Glyph, label: String| {
            div()
                .id((key, u64::from(id)))
                .h(MENU_ITEM_HEIGHT)
                .px(ROW_INSET)
                .flex()
                .items_center()
                .gap(GAP)
                .rounded(RADIUS_SEGMENT)
                .hover(|style| style.bg(theme.hover()))
                .child(Icon::new(glyph).size(ICON_SIZE_SMALL))
                .child(div().min_w_0().truncate().text_size(FONT_SIZE_SMALL).child(label))
        };
        let mute = item("mute", Glyph::BELL_OFF, format!("Mute {app}")).on_click(cx.listener(move |list, _, _, cx| {
            cx.stop_propagation();
            list.menu_for = None;
            model::mute(&app, true, cx);
            cx.notify();
        }));
        let dismiss = item("dismiss", Glyph::CLOSE, "Dismiss".into()).on_click(cx.listener(move |list, _, _, cx| {
            cx.stop_propagation();
            list.menu_for = None;
            model::dismiss(id, cx);
            cx.notify();
        }));
        deferred(
            div()
                .absolute()
                .right(CLOSE_INSET)
                .top(CLOSE_INSET + CLOSE_SIZE + LABEL_NUDGE * 2.0)
                .w(MENU_WIDTH)
                .p(MENU_PAD)
                .rounded(RADIUS_CONTROL)
                .bg(theme.bg)
                .border(DIVIDER_WIDTH)
                .border_color(theme.glass_rim())
                .occlude()
                .child(mute)
                .child(dismiss),
        )
        .with_priority(1)
    }
}

/// How a row shows its notification.
struct Shown {
    /// Its app's name before the summary: not in an open stack, whose
    /// heading names the app.
    app: bool,
    /// A closed stack's top row: a click opens that app's stack.
    stack_of: Option<String>,
}

impl Render for List {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let items = model.items(cx).to_vec();
        let do_not_disturb = model.do_not_disturb(cx);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64);
        // Gone from the list, or down to one: its stack and its menu with it.
        self.expanded.retain(|app| items.iter().filter(|item| &item.app == app).count() > 1);
        self.menu_for = self.menu_for.filter(|id| items.iter().any(|item| item.id == *id));
        let heading = self.heading(items.len(), do_not_disturb, cx);
        let list = div().size_full().body_text(cx).flex().flex_col().gap(GAP).child(heading);
        if items.is_empty() {
            return list.child(Self::empty(cx));
        }
        let found = groups(&items);
        let rows: Vec<AnyElement> = found.iter().map(|group| self.group(group, now, cx)).collect();
        list.child(
            div()
                .id("notifications")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(LIST_GAP)
                .children(rows),
        )
    }
}

/// A card's title: small, in capitals, soft.
fn card_title(text: &str, cx: &App) -> Div {
    div().text_size(FONT_SIZE_SMALL).text_color(Theme::of(cx).fg_soft()).child(text.to_uppercase())
}

/// A quiet text button, like Clear all.
fn text_button(id: impl Into<ElementId>, label: &'static str, cx: &App) -> gpui::Stateful<Div> {
    let hover = Theme::of(cx).hover();
    div()
        .id(id)
        .h(CHIP_HEIGHT)
        .px(GAP)
        .flex()
        .items_center()
        .rounded(RADIUS_SEGMENT)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .text_size(FONT_SIZE_SMALL)
        .font_weight(WEIGHT_LABEL)
        .child(label)
}

/// A number for an app's name, for the ids of its stack's buttons.
fn hash(app: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    app.hash(&mut hasher);
    hasher.finish()
}
