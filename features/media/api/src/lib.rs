//! The media feature's messages: the MPRIS media players on the session
//! bus, the one the bar shows, and their controls, for the now-playing pill
//! and panel.
//!
//! A client sends a [`Command`] (`media_control`, `media_seek`, ...); novad
//! publishes an [`Event`] on [`TOPIC`] whenever a player comes, goes or
//! changes. The work is `nova-media` (`features/media/core`).

use nova_app_info::App;
use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};

/// The topic a client subscribes to for [`Event::Media`].
pub const TOPIC: Topic = Topic::named("media");

/// Sent by the UI. Arguments with unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Plays or pauses the media player `player` (a [`MediaPlayer::id`]),
    /// or skips a track (feature `media`).
    MediaControl { player: String, action: MediaAction },
    /// Moves the player to `position_us` into its track, kept within the
    /// track (feature `media`). Only for a player that `can_seek`.
    MediaSeek { player: String, position_us: i64 },
    /// Shows `player` in the bar instead of the one novad picked, until
    /// another player starts playing (feature `media`).
    MediaShow { player: String },
    /// Brings the player's window forward where it is (feature `media`).
    /// Answers whether it found one to bring.
    MediaFocus { player: String },
    /// The name of the workspace the player's window is on, or `null`
    /// (feature `media`), for the panel's "Go to" row.
    MediaWindow { player: String },
}

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// The media players on the session bus and the one the bar shows
    /// (feature `media`). Topic [`TOPIC`], sent right after
    /// subscribing and whenever a player comes, goes or changes.
    Media(Media),
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

/// The longest player id a media command takes, in bytes: a bus name is at
/// most 255.
pub const MEDIA_PLAYER_MAX_BYTES: usize = 255;

/// The media players (feature `media`): every MPRIS player on the session
/// bus, in the order they came, and the one the bar shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Media {
    pub players: Vec<MediaPlayer>,
    /// The [`MediaPlayer::id`] the bar shows: the one that started playing
    /// last, else the one paused last, unless the user picked another
    /// (`media_show`). `None` when no player plays or is paused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
}

/// One MPRIS player and what it plays. Text is plain and capped in length.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaPlayer {
    /// Its bus name, like `org.mpris.MediaPlayer2.spotify`, which the media
    /// commands take.
    pub id: String,
    /// The installed app it belongs to, from its desktop entry, when novad
    /// found one (and the `launcher` feature is on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<App>,
    /// Its app is a web browser (`WebBrowser` in its entry's categories),
    /// which puts the channel or site where the artist goes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub browser: bool,
    /// The player's own name for itself, like `Spotify`.
    pub identity: String,
    pub status: PlaybackStatus,
    pub title: String,
    /// The artists, joined with ", ".
    pub artist: String,
    pub album: String,
    /// The cover as an image file novad checked or downloaded, when there
    /// is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    /// The track's length, 0 when unknown.
    pub length_us: i64,
    /// Where it was at `position_at_ms` (milliseconds since 1970); it moves
    /// on at `rate` while playing.
    pub position_us: i64,
    pub position_at_ms: i64,
    pub rate: f64,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_seek: bool,
    /// MPRIS `Raise` brings its window forward.
    pub can_raise: bool,
}

/// What a media player is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

/// The now-playing panel's buttons (`media_control`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaAction {
    PlayPause,
    Next,
    Previous,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media() -> Media {
        let spotify = MediaPlayer {
            id: "org.mpris.MediaPlayer2.spotify".into(),
            app: Some(App {
                id: "spotify.desktop".into(),
                name: "Spotify".into(),
                description: Some("Music Player".into()),
                icon: Some("/usr/share/icons/hicolor/256x256/apps/spotify-client.png".into()),
            }),
            browser: false,
            identity: "Spotify".into(),
            status: PlaybackStatus::Playing,
            title: "Aruarian Dance".into(),
            artist: "Nujabes".into(),
            album: "Samurai Champloo: Departure".into(),
            cover: Some("/run/user/1000/nova/media/5f2c0e8a.jpg".into()),
            length_us: 238_000_000,
            position_us: 84_000_000,
            position_at_ms: 1_793_000_000_000,
            rate: 1.0,
            can_next: true,
            can_previous: true,
            can_play: true,
            can_pause: true,
            can_seek: true,
            can_raise: true,
        };
        let firefox = MediaPlayer {
            id: "org.mpris.MediaPlayer2.firefox.instance_1_42".into(),
            app: None,
            browser: true,
            identity: "Mozilla Firefox".into(),
            status: PlaybackStatus::Paused,
            title: "How the scrolling layout in niri works".into(),
            artist: "Linux Desktop Weekly".into(),
            album: String::new(),
            cover: None,
            length_us: 0,
            position_us: 0,
            position_at_ms: 1_793_000_000_000,
            rate: 1.0,
            can_next: false,
            can_previous: false,
            can_play: true,
            can_pause: true,
            can_seek: false,
            can_raise: false,
        };
        Media { players: vec![spotify, firefox], shown: Some("org.mpris.MediaPlayer2.spotify".into()) }
    }

    #[test]
    fn media_rejects_unknown_fields() {
        let media = serde_json::to_value(media()).unwrap();
        assert!(serde_json::from_value::<Media>(media.clone()).is_ok());
        for pointer in ["", "/players/0"] {
            let mut extra = media.clone();
            extra.pointer_mut(pointer).unwrap().as_object_mut().unwrap().insert("extra".into(), 1.into());
            assert!(serde_json::from_value::<Media>(extra).is_err(), "{pointer}");
        }
        let seek = r#"{"v":2,"cmd":"media_seek","args":{"player":"org.mpris.MediaPlayer2.mpv","position_us":1,"x":1}}"#;
        assert!(nova_carrier::decode_request::<Command>(seek).unwrap_err().contains("unknown field"));
        let control = r#"{"v":2,"cmd":"media_control","args":{"player":"p","action":"shuffle"}}"#;
        assert!(nova_carrier::decode_request::<Command>(control).is_err());
    }

    #[test]
    fn its_event_is_on_its_topic() {
        assert_eq!(Event::Media(media()).topic(), Some(TOPIC));
    }
}
