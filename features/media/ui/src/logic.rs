//! What the pill and the panel work out from novad's `media` event, with
//! no drawing in it. novad picks the
//! player to show and checks what it sends; this only finds it, moves its
//! position on while it plays, turns numbers into text, and times the
//! title's scroll and the equalizer's steps.

use nova_media_api::{Media, MediaPlayer, PlaybackStatus};
use std::time::Duration;

const MICROS_PER_MS: f64 = 1000.0;
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3600;

/// The player the bar shows.
pub fn shown(media: &Media) -> Option<&MediaPlayer> {
    find(media, media.shown.as_deref()?)
}

/// The player `id`.
pub fn find<'a>(media: &'a Media, id: &str) -> Option<&'a MediaPlayer> {
    media.players.iter().find(|player| player.id == id)
}

pub fn playing(player: Option<&MediaPlayer>) -> bool {
    player.is_some_and(|player| player.status == PlaybackStatus::Playing)
}

/// Where `player` is in its track at `now_ms` (ms since 1970), in
/// microseconds: where novad last read it, moved on at its rate while it
/// plays, kept within the track.
pub fn position(player: &MediaPlayer, now_ms: i64) -> i64 {
    let mut at = player.position_us as f64;
    if playing(Some(player)) {
        at += (now_ms - player.position_at_ms).max(0) as f64 * MICROS_PER_MS * player.rate;
    }
    if player.length_us > 0 {
        at = at.min(player.length_us as f64);
    }
    at.max(0.0) as i64
}

/// How far through its track `player` is at `now_ms`, 0..1; 0 when its
/// length is unknown.
pub fn progress(player: &MediaPlayer, now_ms: i64) -> f32 {
    if player.length_us <= 0 {
        return 0.0;
    }
    (position(player, now_ms) as f64 / player.length_us as f64) as f32
}

/// A position or length in microseconds as "3:58", or "1:02:03".
pub fn time(micros: i64) -> String {
    let total = u64::try_from(micros / 1_000_000).unwrap_or(0);
    let (hours, minutes, seconds) =
        (total / SECONDS_PER_HOUR, total / SECONDS_PER_MINUTE % 60, total % SECONDS_PER_MINUTE);
    if hours > 0 { format!("{hours}:{minutes:02}:{seconds:02}") } else { format!("{minutes}:{seconds:02}") }
}

/// How far before the track's end a seek lands at most, in microseconds: a
/// looping player takes a seek to the very end as a restart at 0:00.
const SEEK_END_ROOM_US: i64 = 1_000_000;

/// The position at `fraction` (0..1, kept within) of the way through the
/// track, in microseconds, for `media_seek`; never closer to the end than
/// [`SEEK_END_ROOM_US`].
pub fn seek_target(player: &MediaPlayer, fraction: f32) -> i64 {
    let at = (f64::from(fraction.clamp(0.0, 1.0)) * player.length_us as f64).round() as i64;
    at.min(player.length_us - SEEK_END_ROOM_US).max(0)
}

/// Whether the progress line takes clicks: the player seeks and the
/// track's length is known.
pub fn seekable(player: &MediaPlayer) -> bool {
    player.can_seek && player.length_us > 0
}

/// What the player's app is called: its installed app's name, else what
/// the player calls itself.
pub fn app_name(player: &MediaPlayer) -> &str {
    player.app.as_ref().map_or(&player.identity, |app| &app.name)
}

/// The app's icon file, when novad found one.
pub fn app_icon(player: &MediaPlayer) -> Option<&str> {
    player.app.as_ref()?.icon.as_deref().filter(|icon| icon.starts_with('/'))
}

/// Whether the pill shows the artist: browsers put the channel or site
/// there, so their videos show only the title.
pub fn pill_artist(player: &MediaPlayer) -> bool {
    !player.browser && !player.artist.is_empty()
}

/// The players the panel lists as chips to switch between: every one not
/// stopped, once there are two or more.
pub fn chips(media: &Media) -> Vec<&MediaPlayer> {
    let players: Vec<_> = media.players.iter().filter(|player| player.status != PlaybackStatus::Stopped).collect();
    if players.len() >= 2 { players } else { Vec::new() }
}

/// Whether the pill is wanted out for `player`: one that plays or is
/// paused, unless it has lingered paused.
pub fn out(player: Option<&MediaPlayer>, lingered: bool) -> bool {
    player.is_some_and(|player| player.status != PlaybackStatus::Stopped) && !lingered
}

/// The equalizer's bars' heights, each between `low` and `high` (whole
/// pixels), from `random` (0..1 each call).
pub fn equalizer(count: usize, low: f32, high: f32, mut random: impl FnMut() -> f32) -> Vec<f32> {
    (0..count).map(|_| low + (random() * (high - low)).round()).collect()
}

/// A small random source for the equalizer's steps: they only need to
/// look lively, not be unpredictable.
#[derive(Debug, Clone)]
pub struct Jitter(u64);

impl Jitter {
    pub fn new(seed: u64) -> Self {
        // Xorshift never leaves 0.
        Self(seed | 1)
    }

    /// The next number, 0..1.
    pub fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// How long one scroll through a title `over` px too long takes, at
/// `speed` px a second.
pub fn marquee_duration(over: f32, speed: f32) -> Duration {
    Duration::from_secs_f32(over.max(0.0) / speed)
}

/// How far a title `over` px too long has scrolled `elapsed` into one
/// scroll through: a rest at its start, the way through, a rest at its
/// end. `None` once the scroll is done, and it is back at its start.
pub fn marquee_offset(elapsed: Duration, over: f32, speed: f32, rest: Duration) -> Option<f32> {
    let way = marquee_duration(over, speed);
    if elapsed < rest {
        return Some(0.0);
    }
    let moving = elapsed - rest;
    if moving < way {
        return Some(over * (moving.as_secs_f32() / way.as_secs_f32()));
    }
    (moving < way + rest).then_some(over)
}

/// How long a scroll through stands still from `elapsed` on, resting at
/// either end: no frames are needed until then. `None` while it moves.
pub fn marquee_rest_left(elapsed: Duration, over: f32, speed: f32, rest: Duration) -> Option<Duration> {
    let way = marquee_duration(over, speed);
    if elapsed < rest {
        return Some(rest - elapsed);
    }
    let resting_from = rest + way;
    (elapsed >= resting_from).then(|| (resting_from + rest).saturating_sub(elapsed))
}

/// How much of a scrolled title shows at `x` in its box (0..`room`), when its
/// text is `over` px longer than the box and scrolled by `offset`: it fades out
/// over `fade` at an end where more text lies beyond, and not where its own
/// start or end is in view.
pub fn fade_at(x: f32, room: f32, over: f32, offset: f32, fade: f32) -> f32 {
    if over <= 0.0 {
        return 1.0;
    }
    let (fade_start, fade_end) = (offset.min(fade), (over - offset).min(fade));
    let start = if fade_start > 0.0 { (x / fade_start).clamp(0.0, 1.0) } else { 1.0 };
    let end = if fade_end > 0.0 { ((room - x) / fade_end).clamp(0.0, 1.0) } else { 1.0 };
    start.min(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nova_media_api::MediaPlayer;

    fn player(id: &str, status: PlaybackStatus) -> MediaPlayer {
        MediaPlayer {
            id: id.into(),
            app: None,
            browser: false,
            identity: format!("{id} player"),
            status,
            title: "Aruarian Dance".into(),
            artist: "Nujabes".into(),
            album: String::new(),
            cover: None,
            length_us: 238_000_000,
            position_us: 84_000_000,
            position_at_ms: 1_000_000,
            rate: 1.0,
            can_next: true,
            can_previous: true,
            can_play: true,
            can_pause: true,
            can_seek: true,
            can_raise: true,
        }
    }

    fn media(players: Vec<MediaPlayer>, shown: Option<&str>) -> Media {
        Media { players, shown: shown.map(Into::into) }
    }

    #[test]
    fn the_shown_player_is_the_one_novad_names() {
        let both = media(vec![player("a", PlaybackStatus::Playing), player("b", PlaybackStatus::Paused)], Some("b"));
        assert_eq!(shown(&both).map(|player| player.id.as_str()), Some("b"));
        assert!(shown(&media(both.players.clone(), Some("gone"))).is_none());
        assert!(shown(&media(both.players, None)).is_none());
    }

    #[test]
    fn a_playing_position_moves_on_at_its_rate_within_the_track() {
        let mut playing = player("a", PlaybackStatus::Playing);
        assert_eq!(position(&playing, 1_000_000), 84_000_000);
        assert_eq!(position(&playing, 1_002_000), 86_000_000);
        playing.rate = 2.0;
        assert_eq!(position(&playing, 1_002_000), 88_000_000);
        // Never before where novad read it, nor past the end.
        assert_eq!(position(&playing, 999_000), 84_000_000);
        assert_eq!(position(&playing, 9_000_000), 238_000_000);
        let paused = player("a", PlaybackStatus::Paused);
        assert_eq!(position(&paused, 1_002_000), 84_000_000);
    }

    #[test]
    fn progress_is_zero_without_a_length() {
        let mut unknown = player("a", PlaybackStatus::Playing);
        unknown.length_us = 0;
        assert_eq!(progress(&unknown, 1_000_000), 0.0);
        assert!(!seekable(&unknown));
        let known = player("a", PlaybackStatus::Paused);
        assert!((progress(&known, 1_000_000) - 84.0 / 238.0).abs() < 1e-6);
        assert!(seekable(&known));
    }

    #[test]
    fn times_read_as_a_stopwatch_does() {
        assert_eq!(time(0), "0:00");
        assert_eq!(time(238_000_000), "3:58");
        assert_eq!(time(3_723_000_000), "1:02:03");
        assert_eq!(time(-5), "0:00");
    }

    #[test]
    fn a_seek_lands_within_the_track() {
        let known = player("a", PlaybackStatus::Playing);
        assert_eq!(seek_target(&known, 0.5), 119_000_000);
        assert_eq!(seek_target(&known, 1.4), 237_000_000, "short of the end");
        assert_eq!(seek_target(&known, 1.0), 237_000_000);
        assert_eq!(seek_target(&known, -1.0), 0);
    }

    #[test]
    fn browsers_and_empty_artists_show_only_the_title() {
        let mut shown = player("a", PlaybackStatus::Playing);
        assert!(pill_artist(&shown));
        shown.browser = true;
        assert!(!pill_artist(&shown));
        shown.browser = false;
        shown.artist.clear();
        assert!(!pill_artist(&shown));
        assert_eq!(app_name(&shown), "a player");
    }

    #[test]
    fn chips_show_with_two_players_that_are_not_stopped() {
        let one = media(vec![player("a", PlaybackStatus::Playing), player("b", PlaybackStatus::Stopped)], None);
        assert!(chips(&one).is_empty());
        let two = media(
            vec![
                player("a", PlaybackStatus::Playing),
                player("b", PlaybackStatus::Stopped),
                player("c", PlaybackStatus::Paused),
            ],
            None,
        );
        let ids: Vec<_> = chips(&two).iter().map(|player| player.id.as_str()).collect();
        assert_eq!(ids, ["a", "c"]);
    }

    #[test]
    fn the_pill_is_out_while_a_player_plays_or_is_paused_and_has_not_lingered() {
        assert!(out(Some(&player("a", PlaybackStatus::Playing)), false));
        assert!(out(Some(&player("a", PlaybackStatus::Paused)), false));
        assert!(!out(Some(&player("a", PlaybackStatus::Paused)), true));
        assert!(!out(Some(&player("a", PlaybackStatus::Stopped)), false));
        assert!(!out(None, false));
    }

    #[test]
    fn equalizer_bars_stay_between_their_heights() {
        let mut jitter = Jitter::new(42);
        for _ in 0..100 {
            let bars = equalizer(4, 3.0, 12.0, || jitter.next());
            assert_eq!(bars.len(), 4);
            assert!(bars.iter().all(|bar| (3.0..=12.0).contains(bar) && bar.fract() == 0.0), "{bars:?}");
        }
        assert_eq!(equalizer(2, 3.0, 12.0, || 0.5), [8.0, 8.0]);
    }

    #[test]
    fn a_marquee_rests_scrolls_through_and_rests_again() {
        let rest = Duration::from_millis(800);
        // 60 px over at 30 px/s: two seconds on the way.
        assert_eq!(marquee_duration(60.0, 30.0), Duration::from_secs(2));
        assert_eq!(marquee_offset(Duration::ZERO, 60.0, 30.0, rest), Some(0.0));
        assert_eq!(marquee_offset(Duration::from_millis(1800), 60.0, 30.0, rest), Some(30.0));
        assert_eq!(marquee_offset(Duration::from_millis(3000), 60.0, 30.0, rest), Some(60.0));
        assert_eq!(marquee_offset(Duration::from_millis(3700), 60.0, 30.0, rest), None);
    }

    #[test]
    fn a_marquee_needs_no_frames_while_it_rests() {
        let rest = Duration::from_millis(800);
        let left = |ms| marquee_rest_left(Duration::from_millis(ms), 60.0, 30.0, rest);
        assert_eq!(left(0), Some(rest));
        assert_eq!(left(300), Some(Duration::from_millis(500)));
        assert_eq!(left(800), None);
        assert_eq!(left(2799), None);
        assert_eq!(left(3000), Some(Duration::from_millis(600)));
    }

    #[test]
    fn a_long_title_fades_only_where_more_of_it_lies_beyond() {
        // At its start: no fade at the left, a fade at the right end.
        assert_eq!(fade_at(0.0, 150.0, 60.0, 0.0, 18.0), 1.0);
        assert_eq!(fade_at(150.0, 150.0, 60.0, 0.0, 18.0), 0.0);
        assert!((fade_at(141.0, 150.0, 60.0, 0.0, 18.0) - 0.5).abs() < 1e-6);
        // Scrolled to its end: the other way round.
        assert_eq!(fade_at(0.0, 150.0, 60.0, 60.0, 18.0), 0.0);
        assert_eq!(fade_at(150.0, 150.0, 60.0, 60.0, 18.0), 1.0);
        // A title that fits never fades.
        assert_eq!(fade_at(150.0, 150.0, 0.0, 0.0, 18.0), 1.0);
    }
}
