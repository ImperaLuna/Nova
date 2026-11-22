//! The apps installed, as their desktop entries describe them: reading the
//! entries (`desktop_entry`), finding them in the XDG data folders (`scan`),
//! the index of them all that follows installs (`index`), telling which
//! app a running program is (`program`) and starting them (`launch`). The
//! launcher, tray, notifications and media all name apps from the index,
//! and the socket carries them as [`App`]s.

pub mod desktop_entry;
mod index;
pub mod launch;
mod program;
pub mod scan;

pub use index::{AppIndex, Sources, State};
pub use launch::open;
pub use program::{XEMBED_TRAY_BRIDGE, program_name};

use serde::{Deserialize, Serialize};

/// An app from its desktop entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct App {
    /// Its desktop file id, like `org.mozilla.firefox.desktop`.
    pub id: String,
    /// Its name in the user's language.
    pub name: String,
    /// What kind of app it is or what it's for (its generic name or comment).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Its icon: an absolute path to an image, found in the icon theme
    /// when its entry names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// The longest each of an `app_icon` request's names may be, in
/// characters.
pub const APP_ICON_MAX_CHARS: usize = 512;

/// `XDG_CURRENT_DESKTOP`'s names, like `niri`, which entries show in or not.
pub fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}
