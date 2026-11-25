use crate::lock::{LockScreen, Prompt};
use crate::prompt::{self, PLACEHOLDER};
use crate::tokens::{
    AVATAR_SIZE, DOT_GAP, DOT_SIZE, DURATION_SHAKE, DURATION_UNLOCK, FIELD_HEIGHT, FIELD_WIDTH, FOCUS_OPACITY, GAP,
    INITIAL_SIZE,
};
use gpui::{
    App, AppContext, Context, Div, Entity, FocusHandle, Hsla, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, Styled, Subscription, WeakEntity, Window, WindowHandle, div, px,
};
use nova_look::tokens::{
    DURATION_COLOR, FONT_SIZE_SMALL, FONT_SIZE_TITLE, LINE_HEIGHT_WRAPPED, PAD, RADIUS_CONTROL, STROKE_THIN,
    WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_look::{LookStyled, Spinner, Theme, lock_surface, mix};
use nova_stage::{FrameLog, Screen, Stage};
use std::time::Instant;

pub struct LockSurface {
    screen: Screen,
    lock: WeakEntity<LockScreen>,
    prompt: Option<Prompt>,
    focus: FocusHandle,
    frames: Option<FrameLog>,
    _followed: Option<Subscription>,
}

impl LockSurface {
    pub fn open(screen: &Screen, lock: WeakEntity<LockScreen>, cx: &mut App) -> Option<WindowHandle<Self>> {
        let options = lock_surface(screen.id, screen.size);
        let screen = screen.clone();
        let frames = Stage::logs_frames(cx)
            .then(|| FrameLog::new(format!("{}/lock", screen.name), screen.refresh, Instant::now()));
        let opened = cx.open_window(options, move |_, cx| {
            cx.new(|cx: &mut Context<Self>| {
                let held = lock.upgrade();
                let followed = held.as_ref().map(|lock| cx.observe(lock, Self::follow));
                let prompt = held.and_then(|lock| lock.read(cx).prompt_on(screen.id, cx));
                Self { screen, lock, prompt, focus: cx.focus_handle(), frames, _followed: followed }
            })
        });
        match opened {
            Ok(window) => Some(window),
            Err(error) => {
                eprintln!("ERROR nova-ui: can't open the lock screen's window: {error:#}");
                None
            }
        }
    }

    fn follow(&mut self, lock: Entity<LockScreen>, cx: &mut Context<Self>) {
        let prompt = lock.read(cx).prompt_on(self.screen.id, cx);
        if prompt != self.prompt {
            self.prompt = prompt;
            cx.notify();
        }
    }

    fn key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.lock.update(cx, |lock, cx| lock.key(&event.keystroke, cx));
        cx.stop_propagation();
    }

    fn log_frame(&mut self, moving: bool, now: Instant) {
        if let Some(frames) = &mut self.frames {
            for line in frames.frame(moving, now) {
                eprintln!("INFO nova-ui: {line}");
            }
        }
    }
}

impl Render for LockSurface {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        let now = Instant::now();
        let look = self.prompt.as_ref().map(|prompt| Look::of(prompt, now));
        let moving = look.as_ref().is_some_and(|look| look.moving);
        if moving {
            window.request_animation_frame();
        }
        self.log_frame(moving, now);
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .size_full()
            .bg(Theme::of(cx).backdrop)
            .body_text(cx)
            .flex()
            .items_center()
            .justify_center()
            .children(self.prompt.as_ref().zip(look).map(|(prompt, look)| draw(prompt, &look, cx)))
    }
}

struct Look {
    presence: f32,
    shake: f32,
    ring: f32,
    moving: bool,
}

impl Look {
    fn of(prompt: &Prompt, now: Instant) -> Self {
        let done = |since: Option<Instant>, lasting| since.map_or(1.0, |since| prompt::progress(since, lasting, now));
        let (fade, shake, ring) = (
            prompt.unlocking.map_or(0.0, |since| prompt::progress(since, DURATION_UNLOCK, now)),
            done(prompt.shaken, DURATION_SHAKE),
            done(prompt.ring_changed, DURATION_COLOR),
        );
        let moving = prompt.checking || shake < 1.0 || ring < 1.0 || (prompt.unlocking.is_some() && fade < 1.0);
        Self { presence: 1.0 - fade, shake, ring, moving }
    }
}

fn draw(prompt: &Prompt, look: &Look, cx: &App) -> Div {
    let theme = Theme::of(cx);
    let initial: String = prompt.user.chars().take(1).flat_map(char::to_uppercase).collect();
    let avatar = div()
        .size(AVATAR_SIZE)
        .rounded_full()
        .bg(theme.surface_raised())
        .border(STROKE_THIN)
        .border_color(theme.outline())
        .flex()
        .items_center()
        .justify_center()
        .text_size(INITIAL_SIZE)
        .font_weight(WEIGHT_LABEL)
        .text_color(theme.fg_dim())
        .child(initial);
    let name = div().text_size(FONT_SIZE_TITLE).font_weight(WEIGHT_TITLE).child(prompt.user.clone());
    let message = div()
        .h(FONT_SIZE_SMALL * LINE_HEIGHT_WRAPPED)
        .text_size(FONT_SIZE_SMALL)
        .text_color(if prompt.failed { theme.error } else { theme.fg_dim() })
        .children(prompt.message);
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(GAP)
        .opacity(look.presence)
        .child(avatar)
        .child(name)
        .child(field(prompt, look, cx))
        .child(message)
}

fn field(prompt: &Prompt, look: &Look, cx: &App) -> Div {
    let theme = Theme::of(cx);
    let (focused, wrong) = (theme.primary.opacity(FOCUS_OPACITY), theme.error);
    let ring: Hsla = if prompt.failed { mix(focused, wrong, look.ring) } else { mix(wrong, focused, look.ring) };
    let dot = || div().size(DOT_SIZE).rounded_full().bg(theme.fg).flex_none();
    let typed = match prompt.typed {
        0 => div().text_color(theme.fg_dim()).child(PLACEHOLDER),
        typed => div().flex().items_center().gap(DOT_GAP).children((0..prompt::dots(typed)).map(|_| dot())),
    };
    let spinner = prompt.checking.then(|| div().absolute().right(PAD).child(Spinner::new()));
    let field = div()
        .relative()
        .left(prompt::shake(look.shake))
        .size_full()
        .rounded(RADIUS_CONTROL)
        .bg(theme.surface_raised())
        .border(STROKE_THIN)
        .border_color(ring)
        .flex()
        .items_center()
        .pl((PAD - STROKE_THIN).max(px(0.0)))
        .child(typed)
        .children(spinner);
    div().w(FIELD_WIDTH).h(FIELD_HEIGHT).child(field)
}
