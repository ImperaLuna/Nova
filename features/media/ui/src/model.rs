use crate::logic::{self, Jitter};
use crate::measure;
use crate::tokens::{
    BODY_GAP, CHIP_DOT, CHIP_GAP, CHIP_HEIGHT, CHIP_PAD, COVER, DURATION_EQ_STEP, DURATION_LINGER,
    DURATION_PROGRESS_STEP, EQ_BARS, EQ_HIGH, EQ_LOW, GAP, META_GAP, PAD_BOTTOM, PLAY, ROW_GAP, TIMES_GAP, TITLE_LINE,
    TRACK,
};
use gpui::{App, Context, DisplayId, Entity, Pixels, Size, Subscription, Task, size};
use nova_look::motion::Eased;
use nova_look::tokens::{
    BAR_HEIGHT, FONT_SIZE, FONT_SIZE_SMALL, FONT_SIZE_TITLE, ICON_GAP, LINE_HEIGHT, PANEL_PAD, PANEL_WIDTH, ROW_HEIGHT,
    SECTION_GAP, WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_media_api::{Command, Event, Media, MediaAction, MediaPlayer, PlaybackStatus, TOPIC};
use nova_stage::{Screen, Shown, Stage};
use nova_ui_client::{Daemon, Live};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const NAME: &str = "media";
const STILL_EQUALIZER_VAR: &str = "NOVA_STILL_EQUALIZER";
pub const TITLE_LINES: usize = 2;

pub struct NowPlaying {
    pub screen: Screen,
    media: Entity<Live<Media>>,
    current: Option<MediaPlayer>,
    lingered: bool,
    linger: Option<Task<()>>,
    playback: Option<(String, PlaybackStatus)>,
    pub out: Eased,
    pub shown: Shown,
    pub open: bool,
    pub workspace: String,
    asking: Option<Task<()>>,
    pub bars: Vec<f32>,
    jitter: Jitter,
    still: bool,
    stepping: Option<Task<()>>,
    ticking: Option<Task<()>>,
    pub panel_size: Size<Pixels>,
    _followed: Vec<Subscription>,
}

impl NowPlaying {
    pub fn new(screen: &Screen, cx: &mut Context<Self>) -> Self {
        let media = Live::follow(TOPIC, cx, |Event::Media(media)| Some(media));
        let mut followed = vec![cx.observe(&media, |now_playing, _, cx| now_playing.media_changed(cx))];
        followed.extend(Stage::observe_panels(cx, |now_playing, cx| now_playing.follow_open(cx)));
        let still = std::env::var(STILL_EQUALIZER_VAR).is_ok_and(|value| value == "1");
        let seed = SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |since| since.as_nanos() as u64);
        let mut jitter = Jitter::new(seed);
        let bars = if still {
            half_bars()
        } else {
            logic::equalizer(EQ_BARS, EQ_LOW.into(), EQ_HIGH.into(), || jitter.next())
        };
        let mut now_playing = Self {
            screen: screen.clone(),
            media,
            current: None,
            lingered: false,
            linger: None,
            playback: None,
            out: Eased::at(0.0, Instant::now()),
            shown: Shown::new(false),
            open: false,
            workspace: String::new(),
            asking: None,
            bars,
            jitter,
            still,
            stepping: None,
            ticking: None,
            panel_size: size(PANEL_WIDTH, BAR_HEIGHT),
            _followed: followed,
        };
        now_playing.panel_size = now_playing.wanted_size(cx);
        now_playing
    }

    pub fn media<'a>(&self, cx: &'a App) -> Option<&'a Media> {
        self.media.read(cx).get()
    }

    pub fn current(&self) -> Option<&MediaPlayer> {
        self.current.as_ref()
    }

    pub fn feed(&self) -> &Entity<Live<Media>> {
        &self.media
    }

    pub fn playing(&self) -> bool {
        logic::playing(self.current())
    }

    fn media_changed(&mut self, cx: &mut Context<Self>) {
        let shown = self.media(cx).and_then(logic::shown).cloned();
        let id_before = self.current.as_ref().map(|player| player.id.clone());
        let gone = shown.is_none();
        if let Some(player) = shown.clone() {
            self.current = Some(player);
        } else if self.open {
            Stage::close(NAME, self.screen.id, cx);
        }
        let playback = shown.as_ref().map(|player| (player.id.clone(), player.status));
        if playback != self.playback {
            self.playback = playback;
            self.rearm(cx);
        }
        let id_now = shown.as_ref().map(|player| player.id.clone());
        if !gone && id_now != id_before {
            self.workspace.clear();
            if self.open {
                self.ask_workspace(cx);
            }
        }
        self.follow(cx);
    }

    fn follow_open(&mut self, cx: &mut Context<Self>) {
        let open = Stage::is_open(NAME, self.screen.id, cx);
        if open && self.media(cx).and_then(logic::shown).is_none() {
            Stage::close(NAME, self.screen.id, cx);
            return;
        }
        if open == self.open {
            return;
        }
        self.open = open;
        eprintln!("INFO nova-ui: now playing panel {}", if open { "opened" } else { "closed" });
        if open {
            self.workspace.clear();
            self.ask_workspace(cx);
        }
        self.rearm(cx);
        self.follow(cx);
    }

    fn rearm(&mut self, cx: &mut Context<Self>) {
        self.lingered = false;
        let paused =
            self.media(cx).and_then(logic::shown).is_some_and(|player| player.status == PlaybackStatus::Paused);
        self.linger = (paused && !self.open).then(|| {
            cx.spawn(async move |now_playing, cx| {
                cx.background_executor().timer(DURATION_LINGER).await;
                let _ = now_playing.update(cx, |now_playing, cx| {
                    now_playing.lingered = !now_playing.open;
                    now_playing.follow(cx);
                });
            })
        });
    }

    fn ask_workspace(&mut self, cx: &mut Context<Self>) {
        let Some(player) = self.media(cx).and_then(logic::shown).map(|player| player.id.clone()) else { return };
        let answer = Daemon::call::<Option<String>>(Command::MediaWindow { player: player.clone() }, cx);
        self.asking = Some(cx.spawn(async move |now_playing, cx| {
            let workspace = answer.await.unwrap_or_else(|refusal| {
                eprintln!("WARN nova-ui: media: {refusal}");
                None
            });
            let _ = now_playing.update(cx, |now_playing, cx| {
                if now_playing.current.as_ref().is_some_and(|current| current.id == player) {
                    now_playing.workspace = workspace.unwrap_or_default();
                    cx.notify();
                }
            });
        }));
    }

    fn follow(&mut self, cx: &mut Context<Self>) {
        let shown = self.media(cx).and_then(logic::shown);
        let out = logic::out(shown, self.lingered) || self.open;
        if out {
            self.shown.set(true, cx);
        }
        self.out.set_target(if out { 1.0 } else { 0.0 }, Instant::now());
        self.follow_timers(cx);
        let wanted = self.wanted_size(cx);
        if wanted != self.panel_size {
            self.panel_size = wanted;
            let screen = self.screen.id;
            cx.defer(move |cx| Stage::resize(NAME, screen, wanted, cx));
        }
        cx.notify();
    }

    pub fn hide_once_away(&mut self, now: Instant, cx: &mut Context<Self>) -> bool {
        let hidden = self.out.target() == 0.0 && self.out.settled(now) && self.shown.set(false, cx);
        if hidden {
            self.follow_timers(cx);
        }
        hidden
    }

    fn follow_timers(&mut self, cx: &mut Context<Self>) {
        let playing = self.playing();
        let stepping = playing && !self.open && !self.still && self.shown.get();
        if stepping != self.stepping.is_some() {
            self.stepping = stepping.then(|| repeat(DURATION_EQ_STEP, cx, Self::step_bars));
        }
        let ticking = playing && self.open;
        if ticking != self.ticking.is_some() {
            self.ticking = ticking.then(|| repeat(DURATION_PROGRESS_STEP, cx, |_, cx| cx.notify()));
        }
        if self.still {
            self.bars = half_bars();
        }
    }

    fn step_bars(&mut self, _: &mut Context<Self>) {
        let jitter = &mut self.jitter;
        self.bars = logic::equalizer(EQ_BARS, EQ_LOW.into(), EQ_HIGH.into(), || jitter.next());
    }

    pub fn stepping(&self) -> bool {
        self.stepping.is_some()
    }

    fn wanted_size(&self, cx: &App) -> Size<Pixels> {
        let line = |text_size: Pixels| text_size * LINE_HEIGHT;
        let content = PANEL_WIDTH - PANEL_PAD * 2.0;
        let mut meta = Pixels::ZERO;
        if let Some(player) = self.current() {
            let wrap = content - COVER - BODY_GAP;
            let lines = measure::lines(&player.title, (FONT_SIZE_TITLE, WEIGHT_TITLE), wrap, TITLE_LINES, cx);
            meta = line(FONT_SIZE_TITLE) * TITLE_LINE * lines as f32;
            if !player.artist.is_empty() {
                meta += META_GAP + line(FONT_SIZE);
            }
            if !player.album.is_empty() {
                meta += META_GAP + line(FONT_SIZE_SMALL);
            }
        }
        let mut height = BAR_HEIGHT + GAP + COVER.max(meta);
        height += SECTION_GAP + TRACK + TIMES_GAP + line(FONT_SIZE_SMALL);
        height += GAP + PLAY;
        let chip_rows = self.media(cx).map_or(0, |media| chip_rows(media, content, cx));
        if chip_rows > 0 {
            height += ROW_GAP + CHIP_HEIGHT * chip_rows as f32 + CHIP_GAP * (chip_rows - 1) as f32;
        }
        height += ROW_GAP + ROW_HEIGHT + PAD_BOTTOM;
        size(PANEL_WIDTH, height.ceil())
    }

    pub fn control(&self, action: MediaAction, cx: &App) {
        if let Some(player) = self.current() {
            Daemon::send(Command::MediaControl { player: player.id.clone(), action }, cx);
        }
    }

    pub fn seek(&self, position_us: i64, cx: &App) {
        if let Some(player) = self.current() {
            Daemon::send(Command::MediaSeek { player: player.id.clone(), position_us }, cx);
        }
    }

    pub fn show(&self, player: &str, cx: &App) {
        Daemon::send(Command::MediaShow { player: player.to_owned() }, cx);
    }
}

pub fn go_to(player: &str, screen: DisplayId, cx: &mut App) {
    Daemon::send(Command::MediaFocus { player: player.to_owned() }, cx);
    Stage::close(NAME, screen, cx);
}

fn half_bars() -> Vec<f32> {
    logic::equalizer(EQ_BARS, EQ_LOW.into(), EQ_HIGH.into(), || 0.5)
}

fn repeat(
    every: Duration,
    cx: &mut Context<NowPlaying>,
    step: impl Fn(&mut NowPlaying, &mut Context<NowPlaying>) + 'static,
) -> Task<()> {
    cx.spawn(async move |now_playing, cx| {
        loop {
            cx.background_executor().timer(every).await;
            if now_playing.update(cx, |now_playing, cx| step(now_playing, cx)).is_err() {
                break;
            }
        }
    })
}

pub fn chip_width(player: &MediaPlayer, cx: &App) -> Pixels {
    let dot = if logic::playing(Some(player)) { CHIP_DOT + ICON_GAP } else { Pixels::ZERO };
    CHIP_PAD * 2.0 + dot + measure::width(logic::app_name(player), FONT_SIZE_SMALL, WEIGHT_LABEL, cx)
}

fn chip_rows(media: &Media, width: Pixels, cx: &App) -> usize {
    let chips = logic::chips(media);
    if chips.is_empty() {
        return 0;
    }
    let (mut rows, mut used) = (1, Pixels::ZERO);
    for chip in chips {
        let chip = chip_width(chip, cx);
        if used > Pixels::ZERO && used + CHIP_GAP + chip > width {
            rows += 1;
            used = chip;
        } else {
            used += if used > Pixels::ZERO { CHIP_GAP + chip } else { chip };
        }
    }
    rows
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as i64)
}
