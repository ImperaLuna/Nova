//! The now-playing pill's content: the app's icon, the title, the artist
//! and the equalizer (`NowPlaying.qml`'s face). The stage draws the glass
//! around it and opens the panel on a click.
//!
//! It slides out from nothing and back (its width eases with `out`), and
//! eases to a new width when the track changes. A title too long for its
//! room fades out at its end and scrolls through once when the track
//! changes, and again and again while the pill is hovered.

use crate::art::Art;
use crate::logic;
use crate::measure;
use crate::model::NowPlaying;
use crate::tokens::{
    APP_ICON, ART, ARTIST_MAX, DURATION_MARQUEE_REST, EQ_BAR_WIDTH, EQ_BARS, EQ_GAP, EQ_HIGH, EQ_LEAD, EQ_LOW, FADE,
    MARQUEE_SPEED, PAUSED_OPACITY, PILL_START, TITLE_MAX,
};
use gpui::{
    App, Context, Div, Entity, HighlightStyle, InteractiveElement, IntoElement, LineLayout, ParentElement, Pixels,
    Render, SharedString, StatefulInteractiveElement, Styled, StyledText, Task, Window, div, px,
};
use nova_look::motion::Eased;
use nova_look::tokens::{
    BAR_HEIGHT, DURATION_COLOR, DURATION_RESIZE, FONT_SIZE, FONT_SIZE_SMALL, GAP, PAD, WEIGHT_BODY, WEIGHT_TITLE,
};
use nova_look::{Theme, mix};
use nova_media_api::MediaPlayer;
use std::sync::Arc;
use std::time::Instant;

pub struct MediaPill {
    model: Entity<NowPlaying>,
    /// The face's width, easing when the track changes.
    width: Eased,
    /// 1 playing, 0 paused: the icon dims, the title turns dimmer.
    lit: Eased,
    /// The title last drawn, and when its scroll through started.
    title: String,
    scrolled_at: Option<Instant>,
    /// Whether the title could scroll in the last frame.
    was_running: bool,
    hovered: bool,
    /// The pointer just came onto the pill.
    wants_scroll: bool,
    /// The redraw at the end of the rest the title is in, for the scroll
    /// that started then.
    resting: Option<(Instant, Task<()>)>,
}

impl MediaPill {
    pub fn new(model: Entity<NowPlaying>, cx: &mut Context<Self>) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        let now = Instant::now();
        Self {
            model,
            width: Eased::lasting(0.0, DURATION_RESIZE, now),
            lit: Eased::lasting(1.0, DURATION_COLOR, now),
            title: String::new(),
            scrolled_at: None,
            was_running: false,
            hovered: false,
            wants_scroll: false,
            resting: None,
        }
    }

    /// Starts a scroll through, unless it is scrolling already.
    fn scroll(&mut self, running: bool, over: Pixels, now: Instant) {
        if running && over > Pixels::ZERO && self.scrolled_at.is_none() {
            self.scrolled_at = Some(now);
        }
    }

    /// How far the title is scrolled now, following the track, the hover
    /// and whether it may scroll (out all the way, the panel closed).
    fn scrolled(&mut self, title: &str, running: bool, over: Pixels, now: Instant) -> f32 {
        if !running {
            self.scrolled_at = None;
        }
        // A new track scrolls through once.
        if title != self.title {
            self.title = title.to_owned();
            self.scrolled_at = None;
            self.scroll(running, over, now);
        }
        if (running && !self.was_running) || std::mem::take(&mut self.wants_scroll) {
            self.scroll(running, over, now);
        }
        self.was_running = running;
        let Some(started) = self.scrolled_at else { return 0.0 };
        let rest = DURATION_MARQUEE_REST;
        match logic::marquee_offset(now.saturating_duration_since(started), over.into(), MARQUEE_SPEED, rest) {
            Some(offset) => offset,
            None => {
                // Done, and back at its start: again while hovered.
                self.scrolled_at = None;
                if self.hovered {
                    self.scroll(running, over, now);
                }
                0.0
            }
        }
    }
}

impl MediaPill {
    /// Whether the title moves this frame. Resting at an end it stands
    /// still: one redraw is asked for when the rest ends, not every frame.
    fn title_moves(&mut self, over: Pixels, now: Instant, cx: &mut Context<Self>) -> bool {
        let Some(started) = self.scrolled_at else {
            self.resting = None;
            return false;
        };
        let elapsed = now.saturating_duration_since(started);
        let Some(left) = logic::marquee_rest_left(elapsed, over.into(), MARQUEE_SPEED, DURATION_MARQUEE_REST) else {
            self.resting = None;
            return true;
        };
        if self.resting.as_ref().is_none_or(|(of, _)| *of != started) {
            let wake = cx.spawn(async move |pill, cx| {
                cx.background_executor().timer(left).await;
                let _ = pill.update(cx, |pill, cx| {
                    pill.resting = None;
                    cx.notify();
                });
            });
            self.resting = Some((started, wake));
        }
        false
    }
}

impl Render for MediaPill {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        if self.model.update(cx, |model, cx| model.hide_once_away(now, cx)) {
            window.request_animation_frame();
        }
        let model = self.model.read(cx);
        let Some(player) = model.current().cloned() else { return div().id("media-pill") };
        let (open, out) = (model.open, model.out);
        let playing = logic::playing(Some(&player));
        let theme = Theme::of(cx);
        let hover = theme.hover();

        let mut measured = Measured::of(&player, window, cx);
        let (over, face_width) = (measured.over, measured.face_width);
        // Eases to a new track's width once out; sliding, it is the width.
        if out.value(now) < 1.0 {
            self.width = Eased::lasting(face_width.into(), DURATION_RESIZE, now);
        } else {
            self.width.set_target(face_width.into(), now);
        }
        self.lit.set_target(if playing { 1.0 } else { 0.0 }, now);

        let running = out.target() == 1.0 && out.settled(now) && !open;
        measured.offset = self.scrolled(&player.title, running, over, now);
        let moving = !(out.settled(now) && self.width.settled(now) && self.lit.settled(now));
        if moving || self.title_moves(over, now, cx) {
            window.request_animation_frame();
        }

        let face = self.face(&player, &measured, now, cx);
        let shown = out.value(now);
        div()
            .id("media-pill")
            .w(px(self.width.value(now)) * shown)
            .h(BAR_HEIGHT)
            .flex_none()
            .rounded_full()
            .overflow_hidden()
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .on_hover(cx.listener(|pill, hovered: &bool, _, cx| {
                pill.hovered = *hovered;
                // Scrolls through, when it doesn't fit.
                pill.wants_scroll = *hovered;
                cx.notify();
            }))
            .opacity(shown)
            .child(face)
    }
}

impl MediaPill {
    /// The face: the app's icon, the title, the artist and the equalizer.
    fn face(&self, player: &MediaPlayer, measured: &Measured, now: Instant, cx: &App) -> Div {
        let theme = Theme::of(cx);
        let (fg, dim, primary) = (theme.fg, theme.fg_dim(), theme.primary);
        let playing = logic::playing(Some(player));
        // Paused, they lie flat.
        let bars = if playing { self.model.read(cx).bars.clone() } else { vec![EQ_LOW.into(); EQ_BARS] };
        let lit = self.lit.value(now);
        let ink = mix(dim, fg, lit);
        let icon = div()
            .opacity(PAUSED_OPACITY + (1.0 - PAUSED_OPACITY) * lit)
            .child(Art::icon(ART, APP_ICON).build(logic::app_icon(player), cx));
        let title = div()
            .relative()
            .w(measured.title_width)
            .h(FONT_SIZE * nova_look::tokens::LINE_HEIGHT)
            .flex_none()
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(-measured.offset))
                    .whitespace_nowrap()
                    .font_weight(WEIGHT_TITLE)
                    .text_color(ink)
                    .child(faded_title(
                        &player.title,
                        &measured.title,
                        measured.title_width,
                        measured.over,
                        measured.offset,
                    )),
            );
        let artist = measured.artist.map(|width| {
            div()
                .max_w(width)
                .flex_none()
                .truncate()
                .text_size(FONT_SIZE_SMALL)
                .text_color(dim)
                .child(SharedString::from(player.artist.clone()))
        });
        let bar_color = if playing { primary } else { dim };
        let equalizer = div()
            .ml(EQ_LEAD)
            .h(EQ_HIGH)
            .flex()
            .items_end()
            .gap(EQ_GAP)
            .children(bars.into_iter().map(|bar| div().w(EQ_BAR_WIDTH).h(px(bar)).rounded_full().bg(bar_color)));
        div()
            .w(measured.face_width)
            .h(BAR_HEIGHT)
            .flex_none()
            .flex()
            .items_center()
            .gap(GAP)
            .pl(PILL_START)
            .child(icon)
            .child(title)
            .children(artist)
            .child(equalizer)
    }
}

/// How wide the face's parts are.
struct Measured {
    title: Arc<LineLayout>,
    title_width: Pixels,
    /// How much longer than its room the title is.
    over: Pixels,
    artist: Option<Pixels>,
    face_width: Pixels,
    /// How far the title is scrolled.
    offset: f32,
}

impl Measured {
    fn of(player: &MediaPlayer, window: &Window, cx: &App) -> Self {
        let title = measure::line(&player.title, FONT_SIZE, WEIGHT_TITLE, window, cx);
        let title_width = title.width.min(TITLE_MAX);
        let over = (title.width - TITLE_MAX).max(Pixels::ZERO);
        let artist = logic::pill_artist(player)
            .then(|| measure::line(&player.artist, FONT_SIZE_SMALL, WEIGHT_BODY, window, cx).width.min(ARTIST_MAX));
        let bars_width = EQ_BAR_WIDTH * EQ_BARS as f32 + EQ_GAP * (EQ_BARS - 1) as f32;
        let face_width = PILL_START
            + ART
            + GAP
            + title_width
            + artist.map_or(Pixels::ZERO, |artist| GAP + artist)
            + GAP
            + EQ_LEAD
            + bars_width
            + PAD;
        Self { title, title_width, over, artist, face_width, offset: 0.0 }
    }
}

/// The title as drawn, scrolled by `offset`: each letter faded as far as
/// it lies in the fade at an end where more of the title lies beyond.
fn faded_title(text: &str, line: &LineLayout, room: Pixels, over: Pixels, offset: f32) -> StyledText {
    let text = SharedString::from(text.to_owned());
    if over <= Pixels::ZERO {
        return StyledText::new(text);
    }
    let (room, over) = (f32::from(room), f32::from(over));
    let mut faded = Vec::new();
    let mut letters = text.char_indices().peekable();
    while let Some((start, _)) = letters.next() {
        let end = letters.peek().map_or(text.len(), |(next, _)| *next);
        let middle = (f32::from(line.x_for_index(start)) + f32::from(line.x_for_index(end))) / 2.0 - offset;
        let shows = logic::fade_at(middle, room, over, offset, FADE.into());
        if shows < 1.0 {
            faded.push((start..end, HighlightStyle { fade_out: Some(1.0 - shows), ..HighlightStyle::default() }));
        }
    }
    StyledText::new(text).with_highlights(faded)
}
