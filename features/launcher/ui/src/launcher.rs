use crate::order::AppOrder;
use crate::rows::{Layout, Row, completion, rows, step, types_path};
use crate::tokens::{
    EMPTY_HEIGHT, LABEL_HEIGHT, LAUNCHER_GAP, LAUNCHER_PAD, LAUNCHER_TOP, LAUNCHER_WIDTH, ROW_GAP, ROW_HEIGHT, ROWS,
    SEARCH_HEIGHT, TILE_NAME, TILE_SIZE,
};
use gpui::{
    AppContext, Context, Entity, FocusHandle, KeyDownEvent, Pixels, Size, Subscription, Task, Window, px, size,
};
use nova_app_info::App;
use nova_launcher_api::{
    Command, Event, LAUNCHER_SEARCH_MAX_CHARS, Launcher as Apps, LauncherItem, PROVIDER_APPS, TOPIC,
};
use nova_look::motion::Eased;
use nova_look::tokens::{DURATION_FADE, DURATION_FAST, DURATION_PANEL, GAP};
use nova_look::{FieldEvent, TextField};
use nova_nova_files_api::PROVIDER_FILES;
use nova_stage::{Screen, Slots, Stage};
use nova_ui_client::{Daemon, Live, Refusal};
use std::time::Instant;

pub const NAME: &str = "launcher";
const HOST_PROVIDERS: &str = "launcher.providers";

pub fn top(screen: &Screen) -> Pixels {
    (screen.size.height * LAUNCHER_TOP).round()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Running {
    pub provider: String,
    pub id: String,
}

pub struct Launcher {
    pub(crate) screen: Screen,
    pub(crate) field: Entity<TextField>,
    apps: Entity<Live<Apps>>,
    slots: Option<Entity<Slots>>,
    pub(crate) in_files: bool,
    path_switched: bool,
    pub(crate) query: String,
    matches: Vec<LauncherItem>,
    matches_for: String,
    enter_pending: Option<bool>,
    pub(crate) selected: usize,
    pub(crate) running: Option<Running>,
    pub(crate) problem: String,
    shown_recent: Vec<String>,
    pub(crate) rows: Vec<Row>,
    pub(crate) layout: Layout,
    pub(crate) scroll: f32,
    pub(crate) highlight: Eased,
    was_open: bool,
    search: Option<Task<()>>,
    _followed: Vec<Subscription>,
}

impl Launcher {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| TextField::new(cx).max_chars(LAUNCHER_SEARCH_MAX_CHARS));
        let order = AppOrder::of_system();
        let apps = Live::follow(TOPIC, cx, move |Event::Launcher(mut apps)| {
            order.sort(&mut apps.apps);
            Some(apps)
        });
        let slots = Stage::slots(cx);
        let mut followed = vec![
            cx.subscribe(&field, |launcher, _, _: &FieldEvent, cx| launcher.typed(cx)),
            cx.observe(&apps, |launcher, _, cx| launcher.apps_changed(cx)),
        ];
        followed.extend(slots.as_ref().map(|slots| cx.observe(slots, |launcher, _, cx| launcher.modes_changed(cx))));
        Self {
            screen: screen.clone(),
            field,
            apps,
            slots,
            in_files: false,
            path_switched: false,
            query: String::new(),
            matches: Vec::new(),
            matches_for: String::new(),
            enter_pending: None,
            selected: 0,
            running: None,
            problem: String::new(),
            shown_recent: Vec::new(),
            rows: Vec::new(),
            layout: Layout::default(),
            scroll: 0.0,
            highlight: Eased::lasting(0.0, DURATION_FAST, Instant::now()),
            was_open: false,
            search: None,
            _followed: followed,
        }
    }

    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.field.read(cx).focus_handle()
    }

    fn data<'a>(&self, cx: &'a gpui::App) -> Option<&'a Apps> {
        self.apps.read(cx).get()
    }

    pub(crate) fn has_files(&self, cx: &gpui::App) -> bool {
        let providers = self.slots.as_ref().map(|slots| slots.read(cx).for_host(HOST_PROVIDERS, cx));
        providers.is_some_and(|providers| providers.iter().any(|view| view.as_ref() == PROVIDER_FILES))
    }

    pub(crate) fn pinned<'a>(&self, cx: &'a gpui::App) -> Vec<&'a App> {
        let Some(data) = self.data(cx) else { return Vec::new() };
        let fit = ((content_width() + GAP) / (TILE_SIZE + GAP)).floor() as usize;
        crate::rows::pick(&data.apps, &data.pinned).into_iter().take(fit).collect()
    }

    pub(crate) fn shows_pinned(&self, cx: &gpui::App) -> bool {
        !self.in_files && self.query.trim().is_empty() && !self.pinned(cx).is_empty()
    }

    pub(crate) fn is_pinned(&self, id: &str, cx: &gpui::App) -> bool {
        self.data(cx).is_some_and(|data| data.pinned.iter().any(|pinned| pinned == id))
    }

    pub(crate) fn shows_empty(&self) -> bool {
        !self.query.trim().is_empty() && self.rows.is_empty()
    }

    pub(crate) fn is_running(&self, provider: &str, id: &str) -> bool {
        self.running.as_ref().is_some_and(|running| running.provider == provider && running.id == id)
    }

    pub(crate) fn running_selected(&self) -> bool {
        self.rows.get(self.selected).is_some_and(|row| self.is_running(&row.provider, &row.id))
    }

    fn list_room(&self) -> f32 {
        let full = ROW_HEIGHT * ROWS as f32 + ROW_GAP * (ROWS - 1) as f32 + LABEL_HEIGHT * 2.0;
        let around =
            LAUNCHER_PAD * 2.0 + SEARCH_HEIGHT + LAUNCHER_GAP * 4.0 + LABEL_HEIGHT * 2.0 + TILE_SIZE + TILE_NAME;
        let fits = self.screen.size.height - top(&self.screen) - around - LAUNCHER_PAD;
        f32::from(full.min(fits).max(ROW_HEIGHT))
    }

    pub(crate) fn list_height(&self) -> f32 {
        self.layout.height().min(self.list_room())
    }

    pub fn size(&self, cx: &gpui::App) -> Size<Pixels> {
        let mut parts = vec![SEARCH_HEIGHT];
        if self.shows_pinned(cx) {
            parts.extend([LABEL_HEIGHT, TILE_SIZE + TILE_NAME]);
        }
        if !self.problem.is_empty() {
            parts.push(LABEL_HEIGHT);
        }
        if !self.rows.is_empty() {
            parts.push(px(self.list_height()));
        }
        if self.shows_empty() {
            parts.push(EMPTY_HEIGHT);
        }
        let gaps = LAUNCHER_GAP * (parts.len() - 1) as f32;
        size(LAUNCHER_WIDTH, parts.into_iter().fold(LAUNCHER_PAD * 2.0 + gaps, |height, part| height + part))
    }

    fn lay_out(&mut self, cx: &mut Context<Self>) {
        let apps = self.apps.read(cx).get().map(|data| data.apps.as_slice()).unwrap_or_default();
        self.rows = rows(&self.query, &self.matches, &self.shown_recent, apps, self.in_files);
        self.layout = Layout::of(&self.rows);
        self.select(self.selected.min(self.rows.len().saturating_sub(1)), cx);
        let size = self.size(cx);
        let screen = self.screen.id;
        cx.defer(move |cx| Stage::resize(NAME, screen, size, cx));
    }

    pub(crate) fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected = index;
        self.scroll = self.layout.contain(index, self.scroll, self.list_height());
        self.highlight.set_target(self.layout.row_top(index), Instant::now());
        let ghost = self.completion().to_owned();
        self.field.update(cx, |field, cx| field.set_ghost(crate::rows::shown(&ghost), cx));
        cx.notify();
    }

    pub(crate) fn pointed(&mut self, index: usize, cx: &mut Context<Self>) {
        if index != self.selected {
            self.select(index, cx);
        }
    }

    pub(crate) fn scrolled(&mut self, by: f32, cx: &mut Context<Self>) {
        let furthest = self.layout.furthest(self.list_height());
        self.scroll = (self.scroll + by).clamp(0.0, furthest);
        cx.notify();
    }

    fn completion(&self) -> &str {
        if !self.in_files || self.matches_for != self.query {
            return "";
        }
        completion(&self.query, self.rows.get(self.selected))
    }

    pub(crate) fn follow_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open == self.was_open {
            return;
        }
        self.was_open = open;
        eprintln!("INFO nova-ui: launcher {}", if open { "opened" } else { "closed" });
        if open {
            cx.defer_in(window, |launcher, _, cx| launcher.reset(cx));
            return;
        }
        cx.spawn(async move |launcher, cx| {
            cx.background_executor().timer(DURATION_PANEL + DURATION_FADE).await;
            let _ = launcher.update(cx, |launcher, cx| {
                if !launcher.was_open {
                    launcher.reset(cx);
                }
            });
        })
        .detach();
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        self.in_files = false;
        self.path_switched = false;
        self.shown_recent = self.data(cx).map(|data| data.recent.clone()).unwrap_or_default();
        self.query.clear();
        self.clear_matches();
        self.running = None;
        self.scroll = 0.0;
        self.field.update(cx, |field, cx| {
            field.set_text("", cx);
            field.set_placeholder("Search apps", cx);
        });
        self.lay_out(cx);
    }

    fn clear_matches(&mut self) {
        self.matches.clear();
        self.matches_for.clear();
        self.enter_pending = None;
        self.selected = 0;
        self.problem.clear();
        self.search = None;
    }

    fn apps_changed(&mut self, cx: &mut Context<Self>) {
        if self.running.is_none() && (self.was_open || self.shown_recent.is_empty()) {
            self.shown_recent = self.data(cx).map(|data| data.recent.clone()).unwrap_or_default();
        }
        self.lay_out(cx);
    }

    fn modes_changed(&mut self, cx: &mut Context<Self>) {
        if self.in_files && !self.has_files(cx) {
            self.set_mode(false, cx);
        }
        cx.notify();
    }

    fn typed(&mut self, cx: &mut Context<Self>) {
        let text = self.field.read(cx).text().to_owned();
        if text == self.query {
            return;
        }
        if types_path(&text) && !self.in_files {
            self.set_mode(true, cx);
            self.path_switched = self.in_files;
        } else if text.is_empty() && self.path_switched {
            self.set_mode(false, cx);
        }
        self.query = text;
        self.enter_pending = None;
        self.selected = 0;
        self.problem.clear();
        if self.query.trim().is_empty() {
            self.matches.clear();
            self.search = None;
        } else {
            self.ask(cx);
        }
        self.lay_out(cx);
    }

    pub(crate) fn set_mode(&mut self, files: bool, cx: &mut Context<Self>) {
        self.path_switched = false;
        if files == self.in_files || (files && !self.has_files(cx)) {
            return;
        }
        self.in_files = files;
        self.clear_matches();
        let placeholder = if files { "Search files" } else { "Search apps" };
        self.field.update(cx, |field, cx| field.set_placeholder(placeholder, cx));
        if !self.query.trim().is_empty() {
            self.ask(cx);
        }
        self.lay_out(cx);
    }

    fn ask(&mut self, cx: &mut Context<Self>) {
        let (query, files) = (self.query.clone(), self.in_files);
        let mode = files.then(|| PROVIDER_FILES.to_owned());
        let answer = Daemon::call::<Vec<LauncherItem>>(Command::LauncherSearch { query: query.clone(), mode }, cx);
        self.search = Some(cx.spawn(async move |launcher, cx| {
            let found = answer.await.unwrap_or_else(|refusal| {
                eprintln!("WARN nova-ui: launcher search: {refusal}");
                Vec::new()
            });
            let _ = launcher.update(cx, |launcher, cx| launcher.show_matches(&query, files, found, cx));
        }));
    }

    fn show_matches(&mut self, query: &str, files: bool, found: Vec<LauncherItem>, cx: &mut Context<Self>) {
        if query != self.query || files != self.in_files {
            return;
        }
        self.matches = found;
        self.matches_for = query.to_owned();
        self.lay_out(cx);
        if let Some(new_window) = self.enter_pending.take() {
            self.run_row(0, new_window, cx);
        }
    }

    pub(crate) fn run_row(&mut self, index: usize, new_window: bool, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(index) {
            let running = Running { provider: row.provider.clone(), id: row.id.clone() };
            self.run(running, new_window, cx);
        }
    }

    pub(crate) fn run(&mut self, running: Running, new_window: bool, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        self.problem.clear();
        let output = (!self.screen.name.is_empty()).then(|| self.screen.name.to_string());
        let command =
            Command::LauncherRun { provider: running.provider.clone(), id: running.id.clone(), output, new_window };
        let answer = Daemon::call::<()>(command, cx);
        self.running = Some(running);
        let screen = self.screen.id;
        cx.spawn(async move |launcher, cx| {
            let failed = answer.await.err();
            if failed.is_none() {
                cx.background_executor().timer(DURATION_FAST).await;
                cx.update(|cx| Stage::close(NAME, screen, cx));
            }
            let _ = launcher.update(cx, |launcher, cx| launcher.ran(failed, cx));
        })
        .detach();
        self.lay_out(cx);
    }

    fn ran(&mut self, failed: Option<Refusal>, cx: &mut Context<Self>) {
        let Some(refusal) = failed else { return };
        self.problem = match refusal {
            Refusal::Daemon(error) => error,
            other => other.to_string(),
        };
        self.running = None;
        self.shown_recent = self.data(cx).map(|data| data.recent.clone()).unwrap_or_default();
        self.lay_out(cx);
    }

    pub(crate) fn pin(&mut self, id: &str, pinned: bool, cx: &mut Context<Self>) {
        Daemon::send(Command::PinApp { id: id.to_owned(), pinned }, cx);
    }

    pub(crate) fn key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        match event.keystroke.key.as_str() {
            "up" => self.select(step(self.selected, self.rows.len(), -1), cx),
            "down" => self.select(step(self.selected, self.rows.len(), 1), cx),
            "enter" => {
                if !self.query.trim().is_empty() && self.matches_for != self.query {
                    self.enter_pending = Some(modifiers.shift);
                } else {
                    self.run_row(self.selected, modifiers.shift, cx);
                }
            }
            "tab" if !modifiers.shift && !modifiers.control && self.in_files && types_path(&self.query) => {
                let full = self.rows.get(self.selected).and_then(|row| row.file.as_ref());
                if let Some(full) = full.filter(|_| !self.completion().is_empty()).map(|file| file.completion.clone()) {
                    self.field.update(cx, |field, cx| field.set_text(&full, cx));
                }
            }
            "tab" => self.set_mode(!self.in_files, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    pub(crate) fn mode(&self) -> &'static str {
        if self.in_files { PROVIDER_FILES } else { PROVIDER_APPS }
    }
}

pub(crate) fn content_width() -> Pixels {
    LAUNCHER_WIDTH - LAUNCHER_PAD * 2.0
}
