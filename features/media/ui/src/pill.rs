//! The now-playing pill's content: the app's icon, the title, the artist
//! and the equalizer. The stage draws the glass
//! around it and opens the panel on a click.
//!
//! It slides out from nothing and back (its width eases with `out`), and
//! eases to a new width when the track changes. A title too long for its
//! room fades out at its end and scrolls through once when the track
//! changes, and again and again while the pill is hovered.
//!
//! The equalizer eases between the model's steps (`equalizer`), drawn as
//! one smooth shape, at most `EQ_FRAMES_PER_SECOND` whatever the screen's
//! rate: each frame it draws on screen arms a timer for the next, so a
//! pill the stage leaves out, slides away or clips asks for no more. The
//! model's steps redraw nothing: these frames read them while it steps,
//! and stop once it doesn't and the bars have arrived.

use crate::art::Art;
use crate::equalizer::{Levels, outline};
use crate::logic;
use crate::measure;
use crate::model::NowPlaying;
use crate::tokens::{
    APP_ICON, ART, ARTIST_MAX, DURATION_EQ_FALL, DURATION_EQ_RISE, DURATION_MARQUEE_REST, EQ_BAR_WIDTH, EQ_BARS,
    EQ_FRAMES_PER_SECOND, EQ_GAP, EQ_HIGH, EQ_LEAD, EQ_LOW, EQ_OUTLINE_SAMPLES, FADE, MARQUEE_SPEED, PAUSED_OPACITY,
    PILL_START, TITLE_MAX,
};
use gpui::{
    App, Bounds, Context, Div, Entity, HighlightStyle, Hsla, InteractiveElement, IntoElement, LineLayout,
    ParentElement, PathBuilder, Pixels, Render, SharedString, StatefulInteractiveElement, Styled, StyledText, Task,
    Window, canvas, div, point, px,
};
use nova_look::motion::Eased;
use nova_look::tokens::{
    BAR_HEIGHT, DURATION_COLOR, DURATION_RESIZE, FONT_SIZE, FONT_SIZE_SMALL, GAP, PAD, WEIGHT_BODY, WEIGHT_TITLE,
};
use nova_look::{Theme, mix};
use nova_media_api::MediaPlayer;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    /// The equalizer's heights as drawn, easing toward the model's.
    levels: Levels,
    /// Whether the equalizer's last paint was on screen and not clipped
    /// away: only then does it ask for its next frame.
    eq_seen: Rc<Cell<bool>>,
    /// The timer for the equalizer's next frame, while it moves, and when
    /// that frame is due: one step after the one before, so the wait for
    /// the screen's next frame doesn't add up.
    eq_frame: Option<Task<()>>,
    eq_due: Instant,
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
            levels: Levels::default(),
            eq_seen: Rc::new(Cell::new(false)),
            eq_frame: None,
            eq_due: now,
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
        // Paused, the bars lie flat.
        let targets = if playing { model.bars.clone() } else { vec![EQ_LOW.into(); EQ_BARS] };
        let stepping = model.stepping();
        self.levels.ease(&targets, DURATION_EQ_RISE, DURATION_EQ_FALL, now);
        if stepping || !self.levels.settled(&targets) {
            self.next_eq_frame(cx);
        }
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
        let bars = self.levels.heights().to_vec();
        let seen = self.eq_seen.clone();
        let equalizer = div().ml(EQ_LEAD).w(bars_width()).h(EQ_HIGH).flex_none().child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let shows = on_screen(bounds, window);
                    // Back in sight: one frame to start its own frames again.
                    if !seen.replace(shows) && shows {
                        window.request_animation_frame();
                    }
                    paint_equalizer(&bars, bounds, bar_color, window);
                },
            )
            .size_full(),
        );
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

impl MediaPill {
    /// Asks for the equalizer's next frame one `EQ_FRAMES_PER_SECOND` step
    /// from now, if its last paint was seen and none is asked for yet.
    fn next_eq_frame(&mut self, cx: &mut Context<Self>) {
        if self.eq_frame.is_some() || !self.eq_seen.get() {
            return;
        }
        let now = Instant::now();
        // After a pause, or when frames came late, the next is due at once.
        self.eq_due = (self.eq_due + Duration::from_secs(1) / EQ_FRAMES_PER_SECOND).max(now);
        let wait = self.eq_due - now;
        self.eq_frame = Some(cx.spawn(async move |pill, cx| {
            cx.background_executor().timer(wait).await;
            let _ = pill.update(cx, |pill, cx| {
                pill.eq_frame = None;
                cx.notify();
            });
        }));
    }
}

/// Whether `bounds` shows: inside the window and not clipped away by what
/// holds it (the pill sliding out from nothing, or the stage's rows).
fn on_screen(bounds: Bounds<Pixels>, window: &Window) -> bool {
    let screen = Bounds { origin: point(px(0.0), px(0.0)), size: window.viewport_size() };
    let shown = bounds.intersect(&window.content_mask().bounds).intersect(&screen);
    shown.size.width > Pixels::ZERO && shown.size.height > Pixels::ZERO
}

/// How wide the equalizer is: its bars and the gaps between them.
fn bars_width() -> Pixels {
    EQ_BAR_WIDTH * EQ_BARS as f32 + EQ_GAP * (EQ_BARS - 1) as f32
}

/// Fills the equalizer's smooth outline through `bars` (heights in px),
/// standing on the bottom of `bounds`.
fn paint_equalizer(bars: &[f32], bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let shape = outline(bars, EQ_BAR_WIDTH.into(), EQ_GAP.into(), EQ_HIGH.into(), EQ_OUTLINE_SAMPLES);
    let foot = bounds.bottom();
    let points: Vec<_> = shape.iter().map(|(x, y)| point(bounds.origin.x + px(*x), foot - px(*y))).collect();
    let mut path = PathBuilder::fill();
    path.add_polygon(&points, true);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
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
        let face_width = PILL_START
            + ART
            + GAP
            + title_width
            + artist.map_or(Pixels::ZERO, |artist| GAP + artist)
            + GAP
            + EQ_LEAD
            + bars_width()
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
