//! The launcher's panel on one screen: what it holds, and what a key, a
//! click and novad's answers do to it. `draw.rs` renders it.
//!
//! A search field, the pinned apps as tiles, and a list: the recent apps
//! and every other one, or what a search's providers find (apps first: the
//! `launcher.providers` slot order). Up and Down move through the list,
//! Enter runs the selected row (focusing an open app's window where it is;
//! Shift+Enter opens another). With Nova Files on, tabs in the field
//! switch to the Files mode (or Tab, or typing `~` or `/` first): files
//! found by name, or a typed path's folder, whose completion shows dim and
//! Tab takes.
//!
//! The stage owns the glass, the fade in, Esc and a click outside; this
//! view only says how tall it wants to be ([`Stage::resize`]).

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

/// The panel's name: the feature's id, which `nova toggle` uses.
pub const NAME: &str = "launcher";
/// The host of the launcher's providers, whose slots say which modes
/// there are.
const HOST_PROVIDERS: &str = "launcher.providers";

/// How far down `screen` the launcher's top edge is.
pub fn top(screen: &Screen) -> Pixels {
    (screen.size.height * LAUNCHER_TOP).round()
}

/// The row being run.
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
    /// In the Files mode, not the apps'.
    pub(crate) in_files: bool,
    /// Typing a path switched to the Files mode, which clearing the field
    /// undoes.
    path_switched: bool,
    pub(crate) query: String,
    /// What novad's providers found, and the query they answer: until
    /// novad answers the latest, they are the previous query's.
    matches: Vec<LauncherItem>,
    matches_for: String,
    /// Enter pressed before novad answered the latest query: runs its
    /// first match when it does, in a new window when `true`.
    enter_pending: Option<bool>,
    pub(crate) selected: usize,
    pub(crate) running: Option<Running>,
    /// Why the last run failed.
    pub(crate) problem: String,
    /// The recent apps the list shows: novad's latest, held while a row
    /// runs and while the panel is closed, so a started app's move to the
    /// top shows the next time it opens, not as the panel fades out.
    shown_recent: Vec<String>,
    pub(crate) rows: Vec<Row>,
    pub(crate) layout: Layout,
    /// How far the list is scrolled.
    pub(crate) scroll: f32,
    /// Where the selected row's highlight is, in the list's content.
    pub(crate) highlight: Eased,
    was_open: bool,
    /// The search novad hasn't answered yet; a newer one drops it.
    search: Option<Task<()>>,
    _followed: Vec<Subscription>,
}

impl Launcher {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| TextField::new(cx).max_chars(LAUNCHER_SEARCH_MAX_CHARS));
        let apps = Live::follow(TOPIC, cx, |Event::Launcher(apps)| Some(apps));
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

    /// What gets the keyboard when the panel opens.
    pub fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.field.read(cx).focus_handle()
    }

    fn data<'a>(&self, cx: &'a gpui::App) -> Option<&'a Apps> {
        self.apps.read(cx).get()
    }

    /// Nova Files runs: the Files mode is there.
    pub(crate) fn has_files(&self, cx: &gpui::App) -> bool {
        let providers = self.slots.as_ref().map(|slots| slots.read(cx).for_host(HOST_PROVIDERS, cx));
        providers.is_some_and(|providers| providers.iter().any(|view| view.as_ref() == PROVIDER_FILES))
    }

    /// The pinned apps: as many as fit on one row.
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

    /// Nothing matches what's typed.
    pub(crate) fn shows_empty(&self) -> bool {
        !self.query.trim().is_empty() && self.rows.is_empty()
    }

    pub(crate) fn is_running(&self, provider: &str, id: &str) -> bool {
        self.running.as_ref().is_some_and(|running| running.provider == provider && running.id == id)
    }

    /// Whether the selected row is the one running: a pinned tile's run
    /// leaves the list as it is.
    pub(crate) fn running_selected(&self) -> bool {
        self.rows.get(self.selected).is_some_and(|row| self.is_running(&row.provider, &row.id))
    }

    /// The tallest the list gets before it scrolls: [`ROWS`] rows and the
    /// headings of the two groups they may span. On a short screen, fewer:
    /// the panel ends a pad above the screen's edge.
    fn list_room(&self) -> f32 {
        let full = ROW_HEIGHT * ROWS as f32 + ROW_GAP * (ROWS - 1) as f32 + LABEL_HEIGHT * 2.0;
        let around =
            LAUNCHER_PAD * 2.0 + SEARCH_HEIGHT + LAUNCHER_GAP * 4.0 + LABEL_HEIGHT * 2.0 + TILE_SIZE + TILE_NAME;
        let fits = self.screen.size.height - top(&self.screen) - around - LAUNCHER_PAD;
        f32::from(full.min(fits).max(ROW_HEIGHT))
    }

    /// How tall the list is drawn.
    pub(crate) fn list_height(&self) -> f32 {
        self.layout.height().min(self.list_room())
    }

    /// The panel's size for what it shows now.
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

    /// Follows a change to what the list shows: the rows, the selection
    /// within them, the grey completion and the panel's height.
    fn lay_out(&mut self, cx: &mut Context<Self>) {
        let apps = self.apps.read(cx).get().map(|data| data.apps.as_slice()).unwrap_or_default();
        self.rows = rows(&self.query, &self.matches, &self.shown_recent, apps, self.in_files);
        self.layout = Layout::of(&self.rows);
        self.select(self.selected.min(self.rows.len().saturating_sub(1)), cx);
        let size = self.size(cx);
        let screen = self.screen.id;
        // After this update: the stage reads this view to lay the panel out.
        cx.defer(move |cx| Stage::resize(NAME, screen, size, cx));
    }

    /// Selects row `index`, scrolled into view.
    pub(crate) fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected = index;
        self.scroll = self.layout.contain(index, self.scroll, self.list_height());
        self.highlight.set_target(self.layout.row_top(index), Instant::now());
        let ghost = self.completion().to_owned();
        self.field.update(cx, |field, cx| field.set_ghost(crate::rows::shown(&ghost), cx));
        cx.notify();
    }

    /// The pointer moved over row `index`.
    pub(crate) fn pointed(&mut self, index: usize, cx: &mut Context<Self>) {
        if index != self.selected {
            self.select(index, cx);
        }
    }

    /// The wheel turned `by` pixels over the list.
    pub(crate) fn scrolled(&mut self, by: f32, cx: &mut Context<Self>) {
        let furthest = self.layout.furthest(self.list_height());
        self.scroll = (self.scroll + by).clamp(0.0, furthest);
        cx.notify();
    }

    /// What Tab adds to the query: the selected file's path, past what's
    /// typed. Only once novad has answered what's typed: older rows
    /// complete an older query.
    fn completion(&self) -> &str {
        if !self.in_files || self.matches_for != self.query {
            return "";
        }
        completion(&self.query, self.rows.get(self.selected))
    }

    /// Called from `render` with whether the stage has the panel open:
    /// follows it opening and closing.
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
        // Once it has faded out, as it will open: the next time starts at
        // that size instead of easing to it from this one.
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

    /// As it opens: the apps mode, an empty field, the first row.
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

    /// Nova Files stopping takes its mode with it.
    fn modes_changed(&mut self, cx: &mut Context<Self>) {
        if self.in_files && !self.has_files(cx) {
            self.set_mode(false, cx);
        }
        cx.notify();
    }

    /// The field's text changed.
    fn typed(&mut self, cx: &mut Context<Self>) {
        let text = self.field.read(cx).text().to_owned();
        if text == self.query {
            return;
        }
        // A path is typed: the Files mode completes it.
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

    /// Switches to the Files mode or back, searching what's typed in it.
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

    /// Asks novad's providers for what's typed, in this mode.
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

    /// novad's matches for `query`; dropped when something else has been
    /// typed or picked since.
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

    /// Runs row `index`, unless one is running already.
    pub(crate) fn run_row(&mut self, index: usize, new_window: bool, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(index) {
            let running = Running { provider: row.provider.clone(), id: row.id.clone() };
            self.run(running, new_window, cx);
        }
    }

    /// Has novad run what `running` names: an app starts, or its open
    /// window is focused where it is unless `new_window`; a file opens, or
    /// with `new_window` is shown in its folder. The row shows the accent
    /// for a moment, then the panel closes.
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

    /// novad's answer to a run.
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

    /// A key the field left alone. Esc goes on to the stage, which closes
    /// the panel.
    pub(crate) fn key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        match event.keystroke.key.as_str() {
            "up" => self.select(step(self.selected, self.rows.len(), -1), cx),
            "down" => self.select(step(self.selected, self.rows.len(), 1), cx),
            "enter" => {
                // The list still shows the previous query's matches.
                if !self.query.trim().is_empty() && self.matches_for != self.query {
                    self.enter_pending = Some(modifiers.shift);
                } else {
                    self.run_row(self.selected, modifiers.shift, cx);
                }
            }
            // Tab stays in the field and switches modes (Shift+Tab and
            // Ctrl+Tab too). While a path is typed, plain Tab completes it
            // instead, as a shell's would.
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

    /// The mode's key, as the tabs name it.
    pub(crate) fn mode(&self) -> &'static str {
        if self.in_files { PROVIDER_FILES } else { PROVIDER_APPS }
    }
}

/// The width inside the panel's padding.
pub(crate) fn content_width() -> Pixels {
    LAUNCHER_WIDTH - LAUNCHER_PAD * 2.0
}
