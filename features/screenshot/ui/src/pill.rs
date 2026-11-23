use crate::pick::clock;
use crate::tokens::RECORD_DOT;
use gpui::{
    App, Context, Entity, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, div,
};
use nova_look::tokens::{BAR_HEIGHT, FONT_MONO, FONT_SIZE, ICON_GAP, PAD, PILL_BUTTON_SIZE};
use nova_look::{Glyph, Icon, Theme};
use nova_recording_api::{ActiveRecording, Command, Event, TOPIC};
use nova_stage::Shown;
use nova_ui_client::{Daemon, Live};
use std::time::{Duration, Instant};

const SECOND: Duration = Duration::from_secs(1);

pub struct RecordingPill {
    recording: Entity<Live<Option<ActiveRecording>>>,
    since: Instant,
    shown: Shown,
    hovered: bool,
    ticking: Option<Task<()>>,
    _followed: Subscription,
}

impl RecordingPill {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let recording = Live::follow(TOPIC, cx, |event| match event {
            Event::Recording(active) => Some(active),
            _ => None,
        });
        let followed = cx.observe(&recording, |pill: &mut Self, _, cx| pill.follow(cx));
        Self {
            recording,
            since: Instant::now(),
            shown: Shown::new(false),
            hovered: false,
            ticking: None,
            _followed: followed,
        }
    }

    pub fn shown(&self) -> Shown {
        self.shown.clone()
    }

    fn active(&self, cx: &App) -> Option<ActiveRecording> {
        self.recording.read(cx).get().cloned().flatten()
    }

    fn follow(&mut self, cx: &mut Context<Self>) {
        self.since = Instant::now();
        let active = self.active(cx);
        self.shown.set(active.is_some(), cx);
        if active.is_none() {
            self.hovered = false;
        }
        self.ticking = active.filter(|active| !active.paused).map(|active| {
            cx.spawn(async move |pill, cx| {
                let into = Duration::from_millis(active.elapsed_ms % 1000);
                let mut wait = SECOND.saturating_sub(into);
                let mut alive = true;
                while alive {
                    cx.background_executor().timer(wait).await;
                    wait = SECOND;
                    alive = pill.update(cx, |_, cx| cx.notify()).is_ok();
                }
            })
        });
        cx.notify();
    }
}

impl Render for RecordingPill {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(active) = self.active(cx) else { return div().id("recording-pill") };
        let theme = Theme::of(cx);
        let running = if active.paused { Duration::ZERO } else { self.since.elapsed() };
        let elapsed = active.elapsed_ms + running.as_millis() as u64;
        let (dot, ink) = if active.paused { (theme.fg_dim(), theme.fg_dim()) } else { (theme.error, theme.fg) };
        let paused = active.paused;
        let pause = (self.hovered && active.can_pause).then(|| {
            button("pause", if paused { Glyph::PLAY } else { Glyph::PAUSE }, theme.fg, cx).on_mouse_down(
                MouseButton::Left,
                move |_, _, cx| {
                    cx.stop_propagation();
                    Daemon::send(Command::PauseRecording { paused: !paused }, cx);
                },
            )
        });
        let stop = self.hovered.then(|| {
            button("stop", Glyph::STOP, theme.error, cx).on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
                Daemon::send(Command::StopRecording, cx);
            })
        });
        div()
            .id("recording-pill")
            .h(BAR_HEIGHT)
            .px(PAD)
            .flex()
            .items_center()
            .gap(ICON_GAP)
            .cursor_pointer()
            .on_hover(cx.listener(|pill, hovered: &bool, _, cx| {
                if pill.hovered != *hovered {
                    pill.hovered = *hovered;
                    cx.notify();
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| Daemon::send(Command::StopRecording, cx))
            .child(div().flex_none().size(RECORD_DOT).rounded_full().bg(dot))
            .child(div().font_family(FONT_MONO).text_size(FONT_SIZE).text_color(ink).child(clock(elapsed)))
            .children(pause)
            .children(stop)
    }
}

fn button(id: &'static str, glyph: Glyph, ink: Hsla, cx: &App) -> gpui::Stateful<gpui::Div> {
    let hover = Theme::of(cx).hover();
    div()
        .id(id)
        .flex_none()
        .size(PILL_BUTTON_SIZE)
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .hover(move |style| style.bg(hover))
        .child(Icon::new(glyph).color(ink))
}
