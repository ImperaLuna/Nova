use crate::NAME;
use crate::choice::{Answer, Choice, ENTRIES, Key, Pick, entry};
use crate::tokens::{MENU_WIDTH, ROW_GAP};
use gpui::{
    Context, FocusHandle, IndentAdjustment, KeyDownEvent, LineFragment, Pixels, Size, Subscription, Task, Window, font,
    px, size,
};
use nova_look::tokens::{
    DURATION_FADE, DURATION_PANEL, FONT_SANS, FONT_SIZE_SMALL, LABEL_NUDGE, LINE_HEIGHT, LIST_ROW_HEIGHT, PAD,
};
use nova_power_api::{Command, PowerAction, PowerMenu};
use nova_stage::{Screen, Stage};
use nova_ui_client::{Daemon, Refusal};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MINUTE: Duration = Duration::from_secs(60);

pub struct Menu {
    screen: Screen,
    focus: FocusHandle,
    pub(crate) choice: Choice,
    pub(crate) booted_at: Option<u64>,
    pub(crate) open: bool,
    size: Size<Pixels>,
    asking: Option<Task<()>>,
    minutes: Option<Task<()>>,
    leave: Option<Task<()>>,
    reopen_problem: Option<String>,
    _followed: Vec<Subscription>,
}

impl Menu {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let followed = Stage::observe_panels(cx, |menu, cx| menu.follow_stage(cx)).into_iter().collect();
        let mut menu = Self {
            screen: screen.clone(),
            focus: cx.focus_handle(),
            choice: Choice::default(),
            booted_at: None,
            open: false,
            size: size(MENU_WIDTH, px(0.0)),
            asking: None,
            minutes: None,
            leave: None,
            reopen_problem: None,
            _followed: followed,
        };
        menu.size = menu.size(cx);
        menu
    }

    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    pub fn size(&self, cx: &gpui::App) -> Size<Pixels> {
        let rows = ENTRIES.len() as f32;
        let mut height = PAD * 2.0 + header_height() + (LIST_ROW_HEIGHT + ROW_GAP) * rows;
        if !self.choice.problem.is_empty() {
            let lines = problem_lines(&self.choice.problem, cx) as f32;
            height += ROW_GAP + FONT_SIZE_SMALL * LINE_HEIGHT * lines;
        }
        size(MENU_WIDTH, height)
    }

    fn fit(&mut self, cx: &mut Context<Self>) {
        let wanted = self.size(cx);
        if wanted == self.size {
            return;
        }
        self.size = wanted;
        let screen = self.screen.id;
        cx.defer(move |cx| Stage::resize(NAME, screen, wanted, cx));
    }

    fn follow_stage(&mut self, cx: &mut Context<Self>) {
        let open = Stage::is_open(NAME, self.screen.id, cx);
        if open == self.open {
            return;
        }
        self.open = open;
        eprintln!("INFO nova-ui: power menu {}", if open { "opened" } else { "closed" });
        if open {
            self.choice.reset();
            if let Some(problem) = self.reopen_problem.take() {
                self.choice.problem = problem;
            }
            self.leave = None;
            self.ask_options(cx);
            self.tick_minutes(cx);
            self.fit(cx);
        } else {
            self.choice.disarm();
            self.minutes = None;
            if self.choice.leaving.is_some() {
                self.leave_once_faded(cx);
            }
        }
        cx.notify();
    }

    fn ask_options(&mut self, cx: &mut Context<Self>) {
        let answer = Daemon::call::<PowerMenu>(Command::PowerOptions, cx);
        self.asking = Some(cx.spawn(async move |menu, cx| {
            let answer = answer.await;
            let _ = menu.update(cx, |menu, cx| match answer {
                Ok(answer) => {
                    menu.choice.set_options(answer.options);
                    menu.booted_at = answer.booted_at;
                    cx.notify();
                }
                Err(refusal) => eprintln!("WARN nova-ui: power options: {refusal}"),
            });
        }));
    }

    fn tick_minutes(&mut self, cx: &mut Context<Self>) {
        self.minutes = Some(cx.spawn(async move |menu, cx| {
            loop {
                let into = unix_seconds() % MINUTE.as_secs();
                let left = MINUTE.saturating_sub(Duration::from_secs(into));
                cx.background_executor().timer(left).await;
                if menu.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        }));
    }

    pub(crate) fn hover(&mut self, action: PowerAction, cx: &mut Context<Self>) {
        let before = self.choice.selected;
        self.choice.hover(action, Instant::now());
        if self.choice.selected != before {
            cx.notify();
        }
    }

    pub(crate) fn pick(&mut self, action: PowerAction, cx: &mut Context<Self>) {
        let pick = self.choice.pick(action, Instant::now());
        self.act(pick, cx);
    }

    pub(crate) fn key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let key = match keystroke.key.as_str() {
            "down" => Key::Next,
            "tab" if modifiers.shift => Key::Previous,
            "tab" => Key::Next,
            "up" => Key::Previous,
            "enter" => Key::Enter,
            "escape" => Key::Escape,
            _ if modifiers.control || modifiers.alt || modifiers.platform => Key::Other,
            letter => {
                let mut chars = letter.chars();
                match (chars.next(), chars.next()) {
                    (Some(letter), None) if letter.is_alphabetic() => Key::Letter(letter.to_ascii_lowercase()),
                    _ => Key::Other,
                }
            }
        };
        let Answer::Taken(pick) = self.choice.key(key, event.is_held, Instant::now()) else { return };
        cx.stop_propagation();
        self.act(pick, cx);
    }

    fn act(&mut self, pick: Pick, cx: &mut Context<Self>) {
        match pick {
            Pick::Nothing => {}
            Pick::Run(action) => self.run(action, cx),
            Pick::Leave(action) => {
                eprintln!("INFO nova-ui: power menu: {action:?} once closed");
                let screen = self.screen.id;
                cx.defer(move |cx| Stage::close(NAME, screen, cx));
            }
        }
        cx.notify();
    }

    fn leave_once_faded(&mut self, cx: &mut Context<Self>) {
        self.leave = Some(cx.spawn(async move |menu, cx| {
            cx.background_executor().timer(DURATION_PANEL + DURATION_FADE).await;
            let _ = menu.update(cx, |menu, cx| {
                if let Some(action) = menu.choice.left().filter(|_| !menu.open) {
                    menu.run(action, cx);
                }
            });
        }));
    }

    fn run(&mut self, action: PowerAction, cx: &mut Context<Self>) {
        eprintln!("INFO nova-ui: power menu: {action:?}");
        let answer = Daemon::call::<()>(Command::Power { action }, cx);
        cx.spawn(async move |menu, cx| {
            let failed = answer.await.err().map(|refusal| match refusal {
                Refusal::Daemon(error) => error,
                other => other.to_string(),
            });
            let _ = menu.update(cx, |menu, cx| menu.ran(action, failed, cx));
        })
        .detach();
    }

    fn ran(&mut self, action: PowerAction, failed: Option<String>, cx: &mut Context<Self>) {
        let screen = self.screen.id;
        match &failed {
            Some(problem) => {
                eprintln!("WARN nova-ui: power menu: {action:?}: {problem}");
                if entry(action).leaves {
                    self.reopen_problem = Some(problem.clone());
                    cx.defer(move |cx| Stage::open(NAME, screen, cx));
                }
            }
            None => cx.defer(move |cx| Stage::close(NAME, screen, cx)),
        }
        self.choice.finished(failed);
        self.fit(cx);
        cx.notify();
    }

    pub(crate) fn uptime(&self) -> Option<String> {
        let booted_at = self.booted_at.filter(|booted_at| *booted_at > 0)?;
        let up = unix_seconds().saturating_sub(booted_at);
        Some(nova_look::span::uptime(i64::try_from(up).unwrap_or(i64::MAX)))
    }
}

pub(crate) fn header_height() -> Pixels {
    FONT_SIZE_SMALL * LINE_HEIGHT + LABEL_NUDGE * 2.0
}

pub(crate) fn problem_width() -> Pixels {
    MENU_WIDTH - PAD * 2.0 - (PAD + LABEL_NUDGE) - PAD
}

fn problem_lines(problem: &str, cx: &gpui::App) -> usize {
    let mut wrapper = cx.text_system().line_wrapper(font(FONT_SANS), FONT_SIZE_SMALL);
    let fragments = [LineFragment::text(problem)];
    wrapper.wrap_line(&fragments, problem_width(), IndentAdjustment::NoIndent).count() + 1
}

fn unix_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}
