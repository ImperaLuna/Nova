use crate::launcher::{Launcher, NAME, Running, content_width};
use crate::rows::{Row, file_glyph, shown};
use crate::tokens::{
    EMPTY_HEIGHT, FILE_FAINT_OPACITY, FOLDER_COLOR, HIDDEN_ICON_OPACITY, LABEL_HEIGHT, LAUNCHER_GAP, LAUNCHER_PAD,
    MODES_WIDTH, PIN_BUTTON_SIZE, ROW_HEIGHT, SEARCH_HEIGHT, TILE_ICON, TILE_NAME, TILE_SIZE,
};
use gpui::{
    AnyElement, App, Context, Div, ElementId, FontWeight, HighlightStyle, Hsla, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, ParentElement, Render, ScrollEvent, SharedString, Stateful,
    StatefulInteractiveElement, Styled, StyledText, Window, div, prelude::FluentBuilder as _, px,
};
use nova_app_info::App as InstalledApp;
use nova_launcher_api::PROVIDER_APPS;
use nova_look::tokens::{
    APP_ICON_SIZE, DURATION_FAST, FONT_MONO, FONT_SIZE_HEADING, FONT_SIZE_SMALL, GAP, ICON_SIZE_MENU, ICON_SIZE_SMALL,
    PAD, RADIUS_CARD, RADIUS_ROW, ROW_INSET, WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_look::{AppIcon, Glyph, Hovered, Icon, Segments, Theme, fade, fade_color, scrolled};
use nova_nova_files_api::PROVIDER_FILES;
use nova_stage::Stage;
use std::time::Instant;

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.follow_open(Stage::is_open(NAME, self.screen.id, cx), window, cx);
        let now = Instant::now();
        if !self.highlight.settled(now) {
            window.request_animation_frame();
        }
        let theme = Theme::of(cx);
        let (dim, error) = (theme.fg_dim(), theme.error);
        let pinned = self.shows_pinned(cx);
        let problem = (!self.problem.is_empty()).then(|| {
            let what = if self.in_files { "Couldn't open it" } else { "Couldn't start it" };
            label(error).pr(ROW_INSET).child(div().truncate().child(format!("{what}: {}", self.problem)))
        });
        let empty = self.shows_empty().then(|| {
            let note = format!("Nothing matches “{}”", self.query.trim());
            let note = div().overflow_hidden().whitespace_nowrap().text_ellipsis_middle().child(note);
            div().h(EMPTY_HEIGHT).flex().items_center().justify_center().text_color(dim).child(note)
        });
        div()
            .size_full()
            .p(LAUNCHER_PAD)
            .flex()
            .flex_col()
            .gap(LAUNCHER_GAP)
            .on_key_down(cx.listener(Self::key))
            .child(self.search(cx))
            .children(pinned.then(|| label(dim).child("PINNED")))
            .children(pinned.then(|| self.tiles(window, cx)))
            .children(problem)
            .children((!self.rows.is_empty()).then(|| self.list(now, window, cx)))
            .children(empty)
    }
}

fn label(color: Hsla) -> Div {
    div().h(LABEL_HEIGHT).flex_none().pl(ROW_INSET).flex().items_center().text_size(FONT_SIZE_SMALL).text_color(color)
}

#[derive(Clone, Copy)]
struct RowState {
    running: bool,
    hovered: bool,
}

struct RowButton {
    id: SharedString,
    glyph: Glyph,
    filled: bool,
    lit: Option<Hsla>,
    opacity: f32,
}

impl RowButton {
    fn draw(self, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        let theme = Theme::of(cx).clone();
        let hovered = Hovered::of(SharedString::from(format!("{}-hovered", self.id)), window, cx);
        let on = hovered.is(cx);
        let ink = self.lit.unwrap_or(if on { theme.fg } else { theme.fg_dim() });
        let icon = Icon::new(self.glyph).size(ICON_SIZE_SMALL).color(ink);
        let icon = if self.filled { icon.filled(self.glyph) } else { icon };
        let opacity = fade(SharedString::from(format!("{}-shown", self.id)), self.opacity, DURATION_FAST, window, cx);
        div()
            .id(ElementId::Name(self.id))
            .size(PIN_BUTTON_SIZE)
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .when(on, |button| button.bg(theme.hover()))
            .on_hover(hovered.track())
            .opacity(opacity)
            .child(icon)
    }
}

impl Launcher {
    fn search(&self, cx: &mut Context<Self>) -> Div {
        let theme = Theme::of(cx);
        let (dim, raised) = (theme.fg_dim(), theme.surface_raised());
        let modes = self.has_files(cx).then(|| {
            let launcher = cx.weak_entity();
            let tabs = Segments::new([(PROVIDER_APPS, "Apps"), (PROVIDER_FILES, "Files")], self.mode())
                .filled(false)
                .on_pick(move |key, _, cx| {
                    let files = key.as_ref() == PROVIDER_FILES;
                    let _ = launcher.update(cx, |launcher, cx| launcher.set_mode(files, cx));
                });
            div().w(MODES_WIDTH).flex_none().child(tabs)
        });
        let field = div().flex_1().min_w_0().text_size(FONT_SIZE_HEADING).font_weight(WEIGHT_LABEL);
        div()
            .h(SEARCH_HEIGHT)
            .flex_none()
            .rounded(RADIUS_ROW)
            .bg(raised)
            .px(LAUNCHER_PAD)
            .flex()
            .items_center()
            .gap(LAUNCHER_GAP)
            .child(Icon::new(Glyph::SEARCH).size(ICON_SIZE_MENU).color(dim))
            .child(field.child(self.field.clone()))
            .children(modes)
            .child(div().font_family(FONT_MONO).text_size(FONT_SIZE_SMALL).text_color(dim).child("Esc"))
    }

    fn tiles(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let pinned: Vec<InstalledApp> = self.pinned(cx).into_iter().cloned().collect();
        let tiles: Vec<AnyElement> = pinned.iter().map(|app| self.tile(app, window, cx)).collect();
        let fit = ((content_width() + GAP) / (TILE_SIZE + GAP)).floor();
        let between = if fit > 1.0 { (content_width() - TILE_SIZE * fit) / (fit - 1.0) } else { px(0.0) };
        div().h(TILE_SIZE + TILE_NAME).flex_none().flex().gap(between).children(tiles)
    }

    fn tile(&self, app: &InstalledApp, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let (dim, raised, hover, accent) = (theme.fg_dim(), theme.surface_raised(), theme.hover(), theme.accent_fill());
        let key = format!("tile-{}", app.id);
        let hovered = Hovered::of(SharedString::from(format!("{key}-hovered")), window, cx);
        let running = self.is_running(PROVIDER_APPS, &app.id);
        let (on, shows_name) = (running || hovered.is(cx), hovered.is(cx));
        let wash_color = fade_color(
            SharedString::from(format!("{key}-wash")),
            if running { accent } else { hover },
            DURATION_FAST,
            window,
            cx,
        );
        let wash_opacity =
            fade(SharedString::from(format!("{key}-on")), f32::from(u8::from(on)), DURATION_FAST, window, cx);
        let name_opacity =
            fade(SharedString::from(format!("{key}-name")), f32::from(u8::from(shows_name)), DURATION_FAST, window, cx);
        let wash = div().absolute().inset_0().rounded(RADIUS_CARD).bg(wash_color).opacity(wash_opacity);
        let id = app.id.clone();
        let face = div()
            .relative()
            .size(TILE_SIZE)
            .rounded(RADIUS_CARD)
            .bg(raised)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(wash)
            .child(AppIcon::new(app.id.clone(), app.name.clone()).icon(app.icon.clone()).size(TILE_ICON))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |launcher, _: &MouseDownEvent, _, cx| {
                    launcher.run(Running { provider: PROVIDER_APPS.into(), id: id.clone() }, false, cx);
                }),
            );
        let name = div()
            .absolute()
            .top(TILE_SIZE)
            .left(-LAUNCHER_GAP)
            .w(TILE_SIZE + LAUNCHER_GAP * 2.0)
            .h(TILE_NAME)
            .flex()
            .items_end()
            .justify_center()
            .text_size(FONT_SIZE_SMALL)
            .text_color(dim)
            .opacity(name_opacity)
            .child(div().truncate().child(app.name.clone()));
        div()
            .id(SharedString::from(key))
            .on_hover(hovered.track())
            .relative()
            .w(TILE_SIZE)
            .h(TILE_SIZE + TILE_NAME)
            .flex_none()
            .child(face)
            .child(name)
            .into_any_element()
    }

    fn list(&self, now: Instant, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = Theme::of(cx).clone();
        let (dim, selected, accent) = (theme.fg_dim(), theme.selected(), theme.accent_fill());
        let height = self.list_height();
        let fill = if self.running_selected() { accent } else { selected };
        let fill = fade_color("launcher-highlight", fill, DURATION_FAST, window, cx);
        let highlight = div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(self.highlight.value(now) - self.scroll))
            .h(ROW_HEIGHT)
            .rounded(RADIUS_ROW)
            .bg(fill);
        let shown: Vec<usize> = self.layout.visible(self.scroll, height).collect();
        let items: Vec<Div> = shown
            .into_iter()
            .map(|index| {
                let heading =
                    self.layout.heads(index).then(|| label(dim).child(self.rows[index].group.heading().to_uppercase()));
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(self.layout.top(index) - self.scroll))
                    .children(heading)
                    .child(self.row(index, window, cx))
            })
            .collect();
        div()
            .relative()
            .h(px(height))
            .flex_none()
            .overflow_hidden()
            .on_scroll(cx.listener(|launcher, event: &ScrollEvent, _, cx| {
                let turned = scrolled(event);
                launcher.scrolled(-f32::from(turned.y), cx);
                cx.stop_propagation();
            }))
            .child(highlight)
            .children(items)
    }

    fn row(&self, index: usize, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let row = &self.rows[index];
        let key = format!("row-{index}-{}-{}", row.provider, row.id);
        let hovered = Hovered::of(SharedString::from(format!("{key}-hovered")), window, cx);
        let theme = Theme::of(cx).clone();
        let running = index == self.selected && self.running_selected();
        let hidden = row.file.as_ref().is_some_and(|file| file.hidden);
        let ink = match (running, hidden) {
            (true, _) => theme.on_accent,
            (false, true) => theme.fg.alpha(FILE_FAINT_OPACITY),
            (false, false) => theme.fg,
        };
        let dim = if running { theme.on_accent } else { theme.fg_dim() };
        let enter = div().font_family(FONT_MONO).text_size(FONT_SIZE_SMALL).text_color(dim).child("↵");
        let end = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(PAD)
            .children(self.row_buttons(index, RowState { running, hovered: hovered.is(cx) }, window, cx))
            .child(if index == self.selected { enter } else { enter.opacity(0.0) });
        let description = (!row.description.is_empty()).then(|| {
            let text = if row.file.is_some() { shown(&row.description) } else { row.description.clone() };
            div().flex_1().min_w_0().truncate().text_size(FONT_SIZE_SMALL).text_color(dim).child(text)
        });
        let name = div().min_w_0().truncate().font_weight(WEIGHT_LABEL).text_color(ink).child(name(row, running, cx));
        let text = div().flex_1().min_w_0().flex().items_center().gap(GAP).child(name).children(description);
        div()
            .id(SharedString::from(key))
            .on_hover(hovered.track())
            .h(ROW_HEIGHT)
            .px(ROW_INSET)
            .flex()
            .items_center()
            .gap(PAD)
            .cursor_pointer()
            .on_mouse_move(cx.listener(move |launcher, _, _, cx| launcher.pointed(index, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |launcher, _: &MouseDownEvent, _, cx| {
                    if launcher.running.is_none() {
                        launcher.select(index, cx);
                        launcher.run_row(index, false, cx);
                    }
                }),
            )
            .child(row_icon(row, running, cx))
            .child(text)
            .child(end)
    }

    fn row_buttons(
        &self,
        index: usize,
        state: RowState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Stateful<Div>> {
        let row = &self.rows[index];
        let primary = Theme::of(cx).primary;
        let key = format!("row-{index}-{}-{}", row.provider, row.id);
        let shown = |stays: bool| if !state.running && (stays || state.hovered) { 1.0 } else { 0.0 };
        let mut buttons = Vec::new();
        if row.provider == PROVIDER_APPS {
            let pinned = self.is_pinned(&row.id, cx);
            let pin = RowButton {
                id: format!("{key}-pin").into(),
                glyph: Glyph::PIN,
                filled: pinned,
                lit: pinned.then_some(primary),
                opacity: shown(pinned),
            };
            let id = row.id.clone();
            buttons.push(pin.draw(window, cx).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |launcher, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    launcher.pin(&id, !pinned, cx);
                }),
            ));
        }
        if row.file.is_some() {
            let folder = RowButton {
                id: format!("{key}-folder").into(),
                glyph: Glyph::FOLDER,
                filled: false,
                lit: None,
                opacity: shown(false),
            };
            buttons.push(folder.draw(window, cx).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |launcher, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    launcher.run_row(index, true, cx);
                }),
            ));
        }
        buttons
    }
}

fn row_icon(row: &Row, running: bool, cx: &App) -> Div {
    let theme = Theme::of(cx);
    let slot = div().size(APP_ICON_SIZE).flex_none().flex().items_center().justify_center();
    let Some(file) = &row.file else {
        return slot.child(AppIcon::new(row.id.clone(), row.name.clone()).icon(row.icon.clone()));
    };
    let color = match (running, file.dir) {
        (true, _) => theme.on_accent,
        (false, true) => gpui::rgb(FOLDER_COLOR).into(),
        (false, false) => theme.fg_dim(),
    };
    let icon = Icon::new(file_glyph(file)).size(ICON_SIZE_MENU).color(color);
    if file.hidden { slot.opacity(HIDDEN_ICON_OPACITY).child(icon) } else { slot.child(icon) }
}

fn name(row: &Row, running: bool, cx: &App) -> StyledText {
    let accent = (!running).then_some(Theme::of(cx).primary);
    let mut text = String::new();
    let mut matched = Vec::new();
    for run in &row.runs {
        let start = text.len();
        if row.file.is_some() {
            text.push_str(&shown(&run.text));
        } else {
            text.push_str(&run.text);
        }
        if run.matched {
            let weight: FontWeight = WEIGHT_TITLE;
            matched.push((
                start..text.len(),
                HighlightStyle { color: accent, font_weight: Some(weight), ..HighlightStyle::default() },
            ));
        }
    }
    StyledText::new(SharedString::from(text)).with_highlights(matched)
}
