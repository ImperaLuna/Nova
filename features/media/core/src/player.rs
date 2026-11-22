//! What novad reads from one MPRIS player: the properties of its
//! `org.mpris.MediaPlayer2` and `org.mpris.MediaPlayer2.Player` interfaces,
//! read whole with `GetAll`, turned into plain values. Players differ in
//! the types they send (an artist as a string or a list, a length as any
//! integer), so each value is read loosely, and a missing or odd one is its
//! default. Text is cut to [`MAX_TEXT_CHARS`].

use nova_media_api::PlaybackStatus;
use std::collections::HashMap;
use zbus::Connection;
use zbus::wire::{OwnedObjectPath, OwnedVariant, Variant};

pub const PATH: &str = "/org/mpris/MediaPlayer2";
pub const ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
pub const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// Titles, artists, albums and names are cut to this many characters:
/// more than the panel's two lines can show.
pub const MAX_TEXT_CHARS: usize = 300;
/// The longest art URL kept, in bytes; longer ones (a data URL) mean no cover.
const MAX_ART_URL_BYTES: usize = 16 * 1024;

type Properties = HashMap<String, OwnedVariant>;

/// One read of a player.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub identity: String,
    /// Its desktop entry's id without `.desktop`, as it says.
    pub desktop_entry: Option<String>,
    pub can_raise: bool,
    pub status: PlaybackStatus,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// `mpris:artUrl`, unchecked.
    pub art_url: Option<String>,
    /// `mpris:trackid`, which `SetPosition` needs.
    pub track_id: Option<OwnedObjectPath>,
    pub length_us: i64,
    pub position_us: i64,
    pub rate: f64,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_seek: bool,
}

/// Reads the player on the bus name `bus`.
pub async fn read(connection: &Connection, bus: &str) -> zbus::Result<Read> {
    let root = get_all(connection, bus, ROOT_INTERFACE).await?;
    let player = get_all(connection, bus, PLAYER_INTERFACE).await?;
    Ok(from_properties(&root, &player))
}

async fn get_all(connection: &Connection, bus: &str, interface: &str) -> zbus::Result<Properties> {
    let reply = connection.call_method(Some(bus), PATH, Some(PROPERTIES), "GetAll", &(interface,)).await?;
    reply.body().deserialize()
}

/// A read from the two interfaces' properties.
pub fn from_properties(root: &Properties, player: &Properties) -> Read {
    let metadata: Properties = player
        .get("Metadata")
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| Properties::try_from(value).ok())
        .unwrap_or_default();
    let flag = |properties: &Properties, name: &str| properties.get(name).and_then(|value| bool::try_from(value).ok());
    let can_control = flag(player, "CanControl").unwrap_or(true);
    let can = |name: &str| can_control && flag(player, name).unwrap_or(false);
    let status = match player.get("PlaybackStatus").and_then(text).as_deref() {
        Some("Playing") => PlaybackStatus::Playing,
        Some("Paused") => PlaybackStatus::Paused,
        _ => PlaybackStatus::Stopped,
    };
    Read {
        identity: root.get("Identity").and_then(text).unwrap_or_default(),
        desktop_entry: root.get("DesktopEntry").and_then(text).filter(|entry| !entry.is_empty()),
        can_raise: flag(root, "CanRaise").unwrap_or(false),
        status,
        title: metadata.get("xesam:title").and_then(text).unwrap_or_default(),
        artist: metadata.get("xesam:artist").map(texts).unwrap_or_default().join(", "),
        album: metadata.get("xesam:album").and_then(text).unwrap_or_default(),
        art_url: metadata
            .get("mpris:artUrl")
            .and_then(|value| <&str>::try_from(value).ok())
            .filter(|url| !url.is_empty() && url.len() <= MAX_ART_URL_BYTES)
            .map(str::to_string),
        track_id: metadata.get("mpris:trackid").and_then(track_id),
        length_us: metadata.get("mpris:length").and_then(integer).unwrap_or(0).max(0),
        position_us: player.get("Position").and_then(integer).unwrap_or(0).max(0),
        rate: player
            .get("Rate")
            .and_then(|value| f64::try_from(value).ok())
            .filter(|rate| rate.is_finite())
            .unwrap_or(1.0),
        can_next: can("CanGoNext"),
        can_previous: can("CanGoPrevious"),
        can_play: can("CanPlay"),
        can_pause: can("CanPause"),
        can_seek: can("CanSeek"),
    }
}

/// A string value, cut to [`MAX_TEXT_CHARS`].
fn text(value: &OwnedVariant) -> Option<String> {
    <&str>::try_from(value).ok().map(cut)
}

/// A list of strings, or one string, each cut.
fn texts(value: &OwnedVariant) -> Vec<String> {
    if let Some(one) = text(value) {
        return vec![one];
    }
    let Ok(Variant::Array(array)) = Variant::try_from(value) else { return vec![] };
    let items: Vec<String> =
        array.iter().filter_map(|item| <&str>::try_from(item).ok()).filter(|item| !item.is_empty()).map(cut).collect();
    // Joined, the list is cut again.
    vec![cut(&items.join(", "))]
}

fn cut(text: &str) -> String {
    text.chars().take(MAX_TEXT_CHARS).collect()
}

/// Any integer type players send for microseconds.
fn integer(value: &OwnedVariant) -> Option<i64> {
    i64::try_from(value)
        .ok()
        .or_else(|| u64::try_from(value).ok().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| i32::try_from(value).ok().map(i64::from))
        .or_else(|| u32::try_from(value).ok().map(i64::from))
}

/// The track id MPRIS reserves for "no track", which `SetPosition` ignores.
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

/// `mpris:trackid`: an object path, though some players send a string.
/// None for no track.
fn track_id(value: &OwnedVariant) -> Option<OwnedObjectPath> {
    let path = match <&zbus::wire::ObjectPath>::try_from(value) {
        Ok(path) => path.clone().into(),
        Err(_) => OwnedObjectPath::try_from(<&str>::try_from(value).ok()?).ok()?,
    };
    (path.as_str() != NO_TRACK).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::wire::ObjectPath;

    fn properties(pairs: Vec<(&str, OwnedVariant)>) -> Properties {
        pairs.into_iter().map(|(name, value)| (name.to_string(), value)).collect()
    }

    fn variant<'a>(value: impl Into<Variant<'a>>) -> OwnedVariant {
        OwnedVariant::try_from(value.into()).unwrap()
    }

    #[test]
    fn players_send_values_in_more_than_one_type() {
        let metadata = properties(vec![
            ("xesam:title", variant("Feather")),
            ("xesam:artist", variant(vec!["Nujabes", "Cise Starr"])),
            ("mpris:length", variant(175_000_000u64)),
            ("mpris:trackid", variant("/org/mpris/MediaPlayer2/Track/7")),
        ]);
        let player = properties(vec![
            ("PlaybackStatus", variant("Playing")),
            ("Metadata", OwnedVariant::from(metadata)),
            ("Position", variant(12i32)),
            ("CanSeek", variant(true)),
        ]);
        let root = properties(vec![("Identity", variant("Spotify")), ("DesktopEntry", variant("spotify"))]);
        let read = from_properties(&root, &player);
        assert_eq!(read.title, "Feather");
        assert_eq!(read.artist, "Nujabes, Cise Starr");
        assert_eq!(read.length_us, 175_000_000);
        assert_eq!(read.position_us, 12);
        assert_eq!(read.track_id, Some(ObjectPath::try_from("/org/mpris/MediaPlayer2/Track/7").unwrap().into()));
        assert_eq!(read.status, PlaybackStatus::Playing);
        assert!(read.can_seek && !read.can_next);
        assert_eq!(read.desktop_entry.as_deref(), Some("spotify"));
    }

    #[test]
    fn no_track_is_no_track_id() {
        let metadata = properties(vec![("mpris:trackid", variant(ObjectPath::try_from(NO_TRACK).unwrap()))]);
        let read = from_properties(&Properties::new(), &properties(vec![("Metadata", OwnedVariant::from(metadata))]));
        assert_eq!(read.track_id, None);
    }

    #[test]
    fn a_player_that_cannot_be_controlled_offers_nothing() {
        let player = properties(vec![("CanControl", variant(false)), ("CanGoNext", variant(true))]);
        let read = from_properties(&Properties::new(), &player);
        assert!(!read.can_next);
        assert_eq!(read.status, PlaybackStatus::Stopped);
        assert_eq!(read.rate, 1.0);
    }

    #[test]
    fn long_text_is_cut() {
        let long = "a".repeat(MAX_TEXT_CHARS * 4);
        let metadata = properties(vec![
            ("xesam:title", variant(long.as_str())),
            ("xesam:artist", variant(vec![long.as_str(); 3])),
        ]);
        let read = from_properties(&Properties::new(), &properties(vec![("Metadata", OwnedVariant::from(metadata))]));
        assert_eq!(read.title.chars().count(), MAX_TEXT_CHARS);
        assert_eq!(read.artist.chars().count(), MAX_TEXT_CHARS);
    }
}
