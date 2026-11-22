//! The launcher feature's messages: the apps it can open with the user's
//! pinned and recent ones, what its providers find for a search, and
//! running what they found.
//!
//! A client sends a [`Command`] (`launcher_search`, `app_icon`,
//! `launcher_run`, `pin_app`); novad publishes an [`Event`] on [`TOPIC`]
//! right after subscribing and whenever the apps change. Paths on the
//! socket (the launcher's Files mode) are wire text ([`wire_text`]). The
//! work is `nova-launcher` (`features/launcher/core`).

use nova_app_info::App;
use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};

/// The topic a client subscribes to for [`Event::Launcher`].
pub const TOPIC: Topic = Topic::named("launcher");

/// Sent by the UI. Arguments with unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Replies with what the launcher's providers find for `query`, as
    /// [`LauncherItem`]s (feature `launcher`): the providers in their slot
    /// order, each one's best first. A query that starts with a provider's
    /// trigger goes to the providers with that trigger only, without it. A
    /// blank query finds nothing. With a `mode` (a provider's view id, like
    /// `files`) the launcher is in that provider's own mode, and
    /// it alone answers, given the whole query; without one, providers with
    /// a mode of their own don't answer.
    LauncherSearch {
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<String>,
    },
    /// Replies with an icon (an absolute path to an image) for an app
    /// stream, as PipeWire names it (each may be ""): its installed app's
    /// when one matches (feature `launcher`, matched like a tray icon's app,
    /// Nix's `.<name>-wrapped` programs by their name), else the icon
    /// theme's for the icon name it gives or its program, or null.
    AppIcon {
        /// `application.icon-name`, often its app's id.
        icon_name: String,
        /// `application.process.binary`, its program file.
        program: String,
        /// `application.name`.
        name: String,
    },
    /// Runs what the launcher's provider `provider` found as `id` (feature
    /// `launcher`), on `output` (the focused output when unset). An app
    /// ([`PROVIDER_APPS`]) opens and counts towards the recent ones; when it
    /// already has a window, that window takes focus where it is, unless
    /// `new_window` (Shift+Enter) asks for another one. A file
    /// (`files`) opens in its default app and a folder in Nova
    /// Files; with `new_window`, Nova Files shows either in its folder,
    /// selected.
    LauncherRun {
        provider: String,
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        new_window: bool,
    },
    /// Pins an app to the launcher, at the end, or unpins it (feature
    /// `launcher`). Remembered across restarts.
    PinApp { id: String, pinned: bool },
}

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// The apps the launcher can open, and the user's pinned and recent ones
    /// (feature `launcher`). Topic [`TOPIC`], sent right after
    /// subscribing and whenever any of it changes.
    Launcher(Launcher),
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

/// What the launcher shows (feature `launcher`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Launcher {
    /// Every app it can open, sorted by name.
    pub apps: Vec<App>,
    /// The ids of the apps the user pinned, in the order they pinned them.
    pub pinned: Vec<String>,
    /// The ids of the apps launched lately, the latest first.
    pub recent: Vec<String>,
}

/// Matches a provider returns for a search, at most.
pub const MAX_MATCHES: usize = 50;

/// The longest `launcher_search` query, in characters: anything on the
/// socket is capped, and a path typed into the Files mode (each byte at
/// most a character) is the longest anyone enters.
pub const LAUNCHER_SEARCH_MAX_CHARS: usize = PATH_MAX_BYTES;

/// The longest path, in bytes: Linux's own limit.
pub const PATH_MAX_BYTES: usize = 4096;

/// Where the characters that stand for raw bytes start ([`path_to_wire`]):
/// byte `b` (0x80 to 0xFF) is `RAW_BYTE_BASE + b`, in the Private Use Area.
pub const RAW_BYTE_BASE: u32 = 0xF700;

/// A file name or path as wire text, losslessly. Linux
/// names are bytes and needn't be UTF-8 (old archives, Windows drives), but
/// JSON strings must be. Valid UTF-8 is sent as it is; every other byte,
/// and each byte of a character that would read as one of these, becomes
/// the character [`RAW_BYTE_BASE`] + byte. [`path_from_wire`] undoes it, so
/// a name the listing sent names the same file when it comes back, and
/// names that look alike stay apart. `/` and every ASCII byte are kept, so
/// the UI joins paths and names as plain strings. Every path and name on
/// the socket (the launcher's Files mode) is wire text.
pub fn path_to_wire(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    let raw = |text: &mut String, bytes: &[u8]| {
        text.extend(bytes.iter().filter_map(|&byte| char::from_u32(RAW_BYTE_BASE + u32::from(byte))));
    };
    for chunk in bytes.utf8_chunks() {
        for character in chunk.valid().chars() {
            if raw_byte(character).is_some() {
                raw(&mut text, character.encode_utf8(&mut [0; 4]).as_bytes());
            } else {
                text.push(character);
            }
        }
        raw(&mut text, chunk.invalid());
    }
    text
}

/// The bytes of a name or path [`path_to_wire`] sent.
pub fn path_from_wire(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len());
    for character in text.chars() {
        match raw_byte(character) {
            Some(byte) => bytes.push(byte),
            None => bytes.extend_from_slice(character.encode_utf8(&mut [0; 4]).as_bytes()),
        }
    }
    bytes
}

/// A path or name as wire text ([`path_to_wire`]).
pub fn wire_text(path: impl AsRef<std::ffi::OsStr>) -> String {
    use std::os::unix::ffi::OsStrExt;
    path_to_wire(path.as_ref().as_bytes())
}

/// The path wire text names ([`path_from_wire`]).
pub fn wire_path(text: &str) -> std::path::PathBuf {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(path_from_wire(text)).into()
}

/// The byte `character` stands for on the wire, if it stands for one.
fn raw_byte(character: char) -> Option<u8> {
    u8::try_from(u32::from(character).checked_sub(RAW_BYTE_BASE)?).ok().filter(|byte| *byte >= 0x80)
}

/// The launcher provider (`launcher.providers` view) that finds installed
/// apps: the `launcher` feature's own.
pub const PROVIDER_APPS: &str = "apps";

/// Something a launcher provider found for a search, best first within its
/// provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LauncherItem {
    /// The provider that found it (its `launcher.providers` view id), which
    /// runs it.
    pub provider: String,
    /// What it is to its provider, like an app's desktop file id.
    pub id: String,
    /// Its name in runs of text, `matched` on the ones the search matched,
    /// which the launcher highlights. All of it, unmatched, when the search
    /// matched something else, like a keyword.
    pub name: Vec<TextRun>,
    /// A dimmer line beside the name: what it is or does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Its icon: an absolute path to an image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// What it is when it's a file or folder (`files`); then
    /// `id` is its absolute path, and `name` and `description` (its folder)
    /// are wire text ([`wire_text`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<LauncherFile>,
}

/// A file or folder the launcher's Files mode found ([`LauncherItem::file`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LauncherFile {
    /// A folder, or a link to one.
    pub dir: bool,
    /// Its type in the shared MIME database (`inode/directory` for a folder).
    pub mime: String,
    /// Hidden: its name starts with a dot, or a folder it's in does.
    pub hidden: bool,
    /// Its path as the search field spells it, `~/` for Home, a folder's
    /// with a `/` at the end: what Tab completes the query to.
    pub completion: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub matched: bool,
}

/// A stretch of text to highlight, in UTF-16 code units, what QML's
/// strings count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextMark {
    pub start: u32,
    pub length: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_event_is_on_its_topic() {
        let launcher = Launcher { apps: vec![], pinned: vec![], recent: vec![] };
        assert_eq!(Event::Launcher(launcher).topic(), Some(TOPIC));
    }

    #[test]
    fn utf8_paths_go_on_the_wire_as_they_are() {
        for path in ["/home/me/Documents/notes.md", "/tmp/café/ü 1.png", "/", ""] {
            assert_eq!(path_to_wire(path.as_bytes()), path);
            assert_eq!(path_from_wire(path), path.as_bytes());
        }
    }

    /// Latin-1 names, and real characters from the range raw bytes use,
    /// come back as the bytes they were, and names that look alike stay
    /// apart.
    #[test]
    fn other_names_come_back_byte_for_byte() {
        let pua = format!("/x/{}.txt", char::from_u32(RAW_BYTE_BASE + 0xE9).unwrap());
        let names: [&[u8]; 5] = [b"/x/caf\xe9.md", b"/x/caf\xe8.md", b"\xff\xfe/\x80", pua.as_bytes(), b"/x/\xe2\x82"];
        let wire: Vec<String> = names.iter().map(|name| path_to_wire(name)).collect();
        for (name, text) in names.iter().zip(&wire) {
            assert_eq!(path_from_wire(text), *name, "{text:?}");
            assert!(text.starts_with("/x/") == name.starts_with(b"/x/"), "ASCII is kept: {text:?}");
        }
        assert_eq!(wire[0], "/x/caf\u{f7e9}.md");
        let unique: std::collections::HashSet<_> = wire.iter().collect();
        assert_eq!(unique.len(), names.len());
    }
}
