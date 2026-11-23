//! `resources.page`: CPU and memory over the last minutes, and the apps
//! using the most memory now (each with all its processes), from novad's
//! resources events. A search narrows the apps by name; an app's row can
//! end it, after asking under the row. `ResourcesPage.qml` in today's
//! shell.
//!
//! The card watches the machine while this page shows, so the page only
//! follows the events.

use crate::graph::Graph;
use crate::logic::{self, DASH};
use crate::tokens::{
    CONFIRM_BUTTON_HEIGHT, CONFIRM_OPACITY, END_BUTTON_HEIGHT, END_BUTTON_WIDTH, END_HOVER_OPACITY, END_ICON_SIZE,
    LEGEND_DOT, PROCESS_NUMBER_WIDTH, PROCESS_ROW_HEIGHT, SEARCH_HEIGHT,
};
use crate::{QUICK_SETTINGS, logic::matching};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext, Context, Div, ElementId, Entity, Hsla, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div,
};
use nova_look::tokens::{
    CHIP_HEIGHT, FONT_MONO, FONT_SIZE_MICRO, FONT_SIZE_SMALL, GAP, ICON_GAP, ICON_SIZE_SMALL, PAD, QUICK_SETTINGS_GAP,
    RADIUS_CARD, RADIUS_SEGMENT, SEGMENT_PAD, WEIGHT_BODY, WEIGHT_TITLE,
};
use nova_look::{CardTitle, FieldEvent, Glyph, Icon, LookStyled, PageHeader, TextField, Theme};
use nova_resources_api::{AppUsage, Command, Event, Resources, TOPIC};
use nova_stage::Screen;
use nova_ui_client::{Daemon, Live};

pub struct Page {
    screen: Screen,
    resources: Entity<Live<Resources>>,
    search: Entity<TextField>,
    /// What the search holds, trimmed.
    query: String,
    /// The app whose End asks to confirm, by key.
    asked: Option<String>,
    /// The row under the pointer, and the End button under it, by key.
    hovered: Option<String>,
    on_end: Option<String>,
    _followed: Vec<Subscription>,
}

impl Page {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let resources = Live::follow(TOPIC, cx, |Event::Resources(resources)| Some(resources));
        let search = cx.new(|cx| {
            let mut field = TextField::new(cx);
            field.set_placeholder("Search apps", cx);
            field
        });
        let followed = vec![
            cx.observe(&resources, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |page, search, _: &FieldEvent, cx| {
                page.query = search.read(cx).text().trim().to_owned();
                page.asked = None;
                cx.notify();
            }),
        ];
        Self {
            screen: screen.clone(),
            resources,
            search,
            query: String::new(),
            asked: None,
            hovered: None,
            on_end: None,
            _followed: followed,
        }
    }

    /// Esc clears the search first; with none, it goes on to the stage,
    /// which closes the drawer.
    fn key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" && !self.query.is_empty() {
            self.search.update(cx, |search, cx| search.set_text("", cx));
            cx.stop_propagation();
        }
    }

    fn ask(&mut self, key: Option<String>, cx: &mut Context<Self>) {
        self.asked = key;
        cx.notify();
    }

    fn end(&mut self, key: String, cx: &mut Context<Self>) {
        self.asked = None;
        Daemon::send(Command::EndApp { key }, cx);
        cx.notify();
    }

    fn hover(&mut self, row: bool, key: &str, hovered: bool, cx: &mut Context<Self>) {
        let slot = if row { &mut self.hovered } else { &mut self.on_end };
        let now = match (hovered, slot.as_deref() == Some(key)) {
            (true, _) => Some(key.to_owned()),
            (false, true) => None,
            (false, false) => return,
        };
        if *slot != now {
            *slot = now;
            cx.notify();
        }
    }
}

impl Render for Page {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let screen = self.screen.id;
        let reading = self.resources.read(cx).get().cloned();
        div()
            .size_full()
            .body_text(cx)
            .on_key_down(cx.listener(Self::key))
            .flex()
            .flex_col()
            .gap(QUICK_SETTINGS_GAP)
            .child(PageHeader::new("Resources", move |_, _, cx| {
                nova_stage::Stage::show_page(QUICK_SETTINGS, screen, None, cx)
            }))
            .child(graph_card(reading.as_ref(), cx))
            .child(self.apps(reading.as_ref(), cx))
    }
}

/// The graph with its legend above and its time span below.
fn graph_card(reading: Option<&Resources>, cx: &App) -> impl IntoElement + use<> {
    let theme = Theme::of(cx);
    let share = |value: Option<f32>| value.map_or_else(|| DASH.to_owned(), logic::percent);
    let cpu = share(reading.map(|now| now.cpu));
    let memory = share(reading.map(|now| logic::share(now.memory_used, now.memory_total) * 100.0));
    let span = |text: &'static str| {
        div().font_family(FONT_MONO).text_size(FONT_SIZE_MICRO).text_color(theme.fg_dim()).child(text)
    };
    div()
        .flex_none()
        .p(PAD)
        .rounded(RADIUS_CARD)
        .bg(theme.surface_raised())
        .flex()
        .flex_col()
        .gap(GAP)
        .child(
            div()
                .flex()
                .gap(PAD)
                .child(legend(theme.primary, format!("CPU {cpu}")))
                .child(legend(theme.secondary, format!("RAM {memory}"))),
        )
        .child(Graph::new(
            reading.map(|now| now.history.clone()).unwrap_or_default(),
            reading.map_or(1, |now| now.history_size),
        ))
        .child(div().flex().justify_between().child(span("−3 min")).child(span("now")))
}

/// A legend's dot in `color` and its line.
fn legend(color: Hsla, text: String) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(ICON_GAP)
        .child(div().size(LEGEND_DOT).rounded_full().bg(color))
        .child(CardTitle::new(text))
}

/// One number on a row, or a column's head, right-aligned in its column.
fn cell() -> Div {
    div().flex_none().w(PROCESS_NUMBER_WIDTH).flex().justify_end().text_size(FONT_SIZE_SMALL)
}

impl Page {
    /// The search, the columns' heads and the apps under them, scrolling
    /// when there are more.
    fn apps(&self, reading: Option<&Resources>, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = Theme::of(cx);
        let all = reading.map_or(&[][..], |now| now.apps.as_slice());
        let shown = matching(all, &self.query);
        let search = div()
            .flex_none()
            .h(SEARCH_HEIGHT)
            .rounded_full()
            .bg(theme.track())
            .px(PAD)
            .flex()
            .items_center()
            .gap(GAP)
            .text_size(FONT_SIZE_SMALL)
            .child(Icon::new(Glyph::SEARCH).size(ICON_SIZE_SMALL).color(theme.fg_dim()))
            .child(div().flex_1().min_w_0().child(self.search.clone()));
        let dim = theme.fg_dim();
        let heads = div()
            .flex_none()
            .h(CHIP_HEIGHT + GAP)
            .flex()
            .items_end()
            .text_size(FONT_SIZE_SMALL)
            .text_color(dim)
            .child(div().flex_1().pl(PAD).child("Apps by memory"))
            .child(cell().child("CPU"))
            .child(cell().child("Memory"))
            .child(div().flex_none().w(SEGMENT_PAD + END_BUTTON_WIDTH));
        let none = (shown.is_empty() && !self.query.is_empty()).then(|| {
            div()
                .pl(PAD)
                .pt(GAP)
                .text_size(FONT_SIZE_SMALL)
                .text_color(dim)
                .child(format!("No app matches “{}”", self.query))
        });
        let rows: Vec<AnyElement> = shown.into_iter().map(|app| self.row(app, cx)).collect();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(search)
            .child(heads)
            .child(div().id("apps").flex_1().min_h_0().overflow_y_scroll().children(none).children(rows))
    }

    /// An app's row: its name and how many processes, its CPU and memory,
    /// and End (✕) while hovered; asking, the question under it.
    fn row(&self, app: &AppUsage, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let key = app.key.clone();
        let asked = self.asked.as_deref() == Some(key.as_str());
        let hovered = self.hovered.as_deref() == Some(key.as_str());
        let on_end = self.on_end.as_deref() == Some(key.as_str());
        let lit = on_end || asked;
        let end = (!app.protected && (hovered || on_end || asked)).then(|| {
            let (hover_key, click_key) = (key.clone(), key.clone());
            div()
                .id(ElementId::Name(format!("end-{key}").into()))
                .size_full()
                .rounded(RADIUS_SEGMENT)
                .flex()
                .items_center()
                .justify_center()
                .when(lit, |button| button.bg(theme.secondary.alpha(END_HOVER_OPACITY)))
                .child(Icon::new(Glyph::CLOSE).size(END_ICON_SIZE).color(if lit {
                    theme.secondary
                } else {
                    theme.fg_dim()
                }))
                .on_hover(cx.listener(move |page, hovered: &bool, _, cx| page.hover(false, &hover_key, *hovered, cx)))
                .on_click(cx.listener(move |page, _, _, cx| {
                    let ask = (page.asked.as_deref() != Some(click_key.as_str())).then(|| click_key.clone());
                    page.ask(ask, cx);
                }))
        });
        let row_key = key.clone();
        let line = div()
            .id(ElementId::Name(format!("app-{key}").into()))
            .h(PROCESS_ROW_HEIGHT)
            .rounded(RADIUS_SEGMENT)
            .when(hovered || asked, |row| row.bg(theme.surface_raised()))
            .on_hover(cx.listener(move |page, hovered: &bool, _, cx| page.hover(true, &row_key, *hovered, cx)))
            .pl(PAD)
            .pr(SEGMENT_PAD)
            .flex()
            .items_center()
            .text_size(FONT_SIZE_SMALL)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .mr(GAP)
                    .flex()
                    .items_baseline()
                    .gap(ICON_GAP)
                    .child(div().min_w_0().truncate().text_color(theme.fg).child(app.name.clone()))
                    .children(logic::processes(app.processes).map(|count| {
                        div()
                            .flex_none()
                            .font_family(FONT_MONO)
                            .text_size(FONT_SIZE_MICRO)
                            .text_color(theme.fg_dim())
                            .child(count)
                    })),
            )
            .child(cell().font_family(FONT_MONO).text_color(theme.fg).child(logic::cpu(app.cpu)))
            .child(cell().font_family(FONT_MONO).text_color(theme.fg_dim()).child(nova_look::bytes::bytes(app.memory)))
            .child(div().flex_none().w(END_BUTTON_WIDTH).h(END_BUTTON_HEIGHT).children(end));
        let confirm = asked.then(|| self.confirm(app, &theme, cx));
        div().flex().flex_col().child(line).children(confirm).into_any_element()
    }

    /// Asks before ending: the app is asked to close.
    fn confirm(&self, app: &AppUsage, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let key = app.key.clone();
        div()
            .p(PAD)
            .rounded(RADIUS_SEGMENT)
            .bg(theme.secondary.alpha(CONFIRM_OPACITY))
            .flex()
            .items_center()
            .gap(GAP)
            .text_size(FONT_SIZE_SMALL)
            .child(div().flex_1().min_w_0().text_color(theme.fg).child(format!("End {}?", app.name)))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .gap(ICON_GAP)
                    .child(
                        confirm_button("cancel", "Cancel", false, theme)
                            .on_click(cx.listener(|page, _, _, cx| page.ask(None, cx))),
                    )
                    .child(
                        confirm_button("end", "End", true, theme)
                            .on_click(cx.listener(move |page, _, _, cx| page.end(key.clone(), cx))),
                    ),
            )
    }
}

/// Cancel, or End in the second accent.
fn confirm_button(id: &'static str, label: &'static str, accent: bool, theme: &Theme) -> gpui::Stateful<Div> {
    let hover = theme.hover();
    div()
        .id(SharedString::from(id))
        .h(CONFIRM_BUTTON_HEIGHT)
        .px(PAD)
        .rounded(RADIUS_SEGMENT)
        .flex()
        .items_center()
        .font_weight(if accent { WEIGHT_TITLE } else { WEIGHT_BODY })
        .map(|button| match accent {
            true => button.bg(theme.secondary).text_color(theme.on_accent),
            false => button.bg(theme.track()).text_color(theme.fg).hover(move |style| style.bg(hover)),
        })
        .child(label)
}
