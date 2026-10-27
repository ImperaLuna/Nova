//! Reads one MPRIS player into a [`Reading`]: its two property sets
//! (`org.mpris.MediaPlayer2` and `.Player`, from one `GetAll` each), with
//! text cut to a length and stripped of control characters, since any app
//! on the bus can name its track anything.

use super::MAX_TEXT_CHARS;
use nova_protocol::MediaStatus;
use std::collections::HashMap;
use zbus::{OwnedObjectPath, OwnedVariant};

/// What a player says about itself and its track, as read.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub identity: String,
    /// Its desktop file's name, without `.desktop` (`DesktopEntry`).
    pub desktop_entry: String,
    pub can_raise: bool,
    pub status: MediaStatus,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// `mpris:artUrl` as the player gave it, unchecked.
    pub art_url: String,
    /// `mpris:trackid`, which `SetPosition` needs.
    pub track_id: String,
    pub length_us: u64,
    pub position_us: u64,
    pub rate: f64,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_seek: bool,
}

type Properties = HashMap<String, OwnedVariant>;

/// Reads a player's `GetAll` answers. Missing or mistyped properties read
/// as the MPRIS defaults (empty, false, zero), so a half-finished player
/// still shows what it has.
pub fn read(root: &Properties, player: &Properties) -> Reading {
    let metadata: Properties = get(player, "Metadata").unwrap_or_default();
    let artists: Vec<String> = meta(&metadata, "xesam:artist").unwrap_or_default();
    Reading {
        identity: text(get::<String>(root, "Identity").unwrap_or_default()),
        desktop_entry: text(get::<String>(root, "DesktopEntry").unwrap_or_default()),
        can_raise: get(root, "CanRaise").unwrap_or(false),
        status: match get::<String>(player, "PlaybackStatus").as_deref() {
            Some("Playing") => MediaStatus::Playing,
            Some("Paused") => MediaStatus::Paused,
            _ => MediaStatus::Stopped,
        },
        title: text(meta(&metadata, "xesam:title").unwrap_or_default()),
        artist: text(artists.join(", ")),
        album: text(meta(&metadata, "xesam:album").unwrap_or_default()),
        art_url: meta(&metadata, "mpris:artUrl").unwrap_or_default(),
        track_id: meta::<OwnedObjectPath>(&metadata, "mpris:trackid")
            .map(|id| id.as_str().to_string())
            .unwrap_or_default(),
        length_us: meta::<i64>(&metadata, "mpris:length").unwrap_or(0).max(0) as u64,
        position_us: get::<i64>(player, "Position").unwrap_or(0).max(0) as u64,
        rate: get(player, "Rate").unwrap_or(1.0),
        can_next: get(player, "CanGoNext").unwrap_or(false),
        can_previous: get(player, "CanGoPrevious").unwrap_or(false),
        can_play: get(player, "CanPlay").unwrap_or(false),
        can_pause: get(player, "CanPause").unwrap_or(false),
        can_seek: get(player, "CanSeek").unwrap_or(false),
    }
}

/// `text` with at most [`MAX_TEXT_CHARS`] characters, without control
/// characters (a newline in a title would break the two-line clamp).
pub fn text(value: String) -> String {
    value.chars().filter(|c| !c.is_control()).take(MAX_TEXT_CHARS).collect()
}

fn get<T: TryFrom<OwnedVariant>>(properties: &Properties, key: &str) -> Option<T> {
    T::try_from(properties.get(key)?.try_clone().ok()?).ok()
}

fn meta<T: TryFrom<OwnedVariant>>(metadata: &Properties, key: &str) -> Option<T> {
    get(metadata, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::Variant;

    fn owned(value: impl Into<Variant<'static>>) -> OwnedVariant {
        OwnedVariant::try_from(value.into()).unwrap()
    }

    #[test]
    fn reads_track_and_playback() {
        let metadata = HashMap::from([
            ("xesam:title".to_string(), owned("Song")),
            ("xesam:artist".to_string(), owned(vec!["One".to_string(), "Two".to_string()])),
            ("mpris:length".to_string(), owned(200_000_000i64)),
            ("mpris:trackid".to_string(), owned(OwnedObjectPath::try_from("/track/1").unwrap())),
        ]);
        let root = HashMap::from([("Identity".to_string(), owned("Spotify")), ("CanRaise".to_string(), owned(true))]);
        let player = HashMap::from([
            ("PlaybackStatus".to_string(), owned("Paused")),
            ("Metadata".to_string(), owned(Variant::from(metadata))),
            ("Position".to_string(), owned(1_500_000i64)),
            ("CanSeek".to_string(), owned(true)),
        ]);
        let reading = read(&root, &player);
        assert_eq!(reading.title, "Song");
        assert_eq!(reading.artist, "One, Two");
        assert_eq!(reading.length_us, 200_000_000);
        assert_eq!(reading.position_us, 1_500_000);
        assert_eq!(reading.track_id, "/track/1");
        assert_eq!(reading.status, MediaStatus::Paused);
        assert!(reading.can_raise && reading.can_seek && !reading.can_next);
    }

    #[test]
    fn missing_properties_read_as_defaults() {
        let reading = read(&HashMap::new(), &HashMap::new());
        assert_eq!(reading.status, MediaStatus::Stopped);
        assert_eq!(reading.title, "");
        assert_eq!(reading.rate, 1.0);
    }

    #[test]
    fn text_is_capped_and_loses_control_characters() {
        let long = format!("a\nb{}", "c".repeat(MAX_TEXT_CHARS));
        let cleaned = text(long);
        assert_eq!(cleaned.chars().count(), MAX_TEXT_CHARS);
        assert!(cleaned.starts_with("ab"));
    }
}
