//! The tray feature's messages: the apps in the system tray, for the bar's
//! tray and the quick settings tray card.
//!
//! A client sends a [`Command`] (`hide_tray_app`, `activate_tray_item`,
//! `tray_menu_picked`); novad publishes an [`Event`] on [`TOPIC`] right
//! after subscribing and whenever an item comes or goes. The work is
//! `nova-tray` (`features/tray/core`).

use nova_app_info::App;
use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};

/// The topic a client subscribes to for [`Event::Tray`].
pub const TOPIC: Topic = Topic::named("tray");

/// Sent by the UI. Arguments with unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Hides a tray app from the bar, or shows it again (feature `tray`):
    /// `id` is the item's app id, or its own id when it has none (see
    /// [`Tray::hidden`]). Remembered across restarts.
    HideTrayApp { id: String, hidden: bool },
    /// The tray item at `address` (as [`TrayItem::address`]) was clicked
    /// (feature `tray`): it's asked to show its window, which then takes
    /// focus.
    ActivateTrayItem { address: String },
    /// An entry of the menu of the tray item at `address` was picked
    /// (feature `tray`): a window its app shows next takes focus, as it
    /// gets none from a menu either.
    TrayMenuPicked { address: String },
}

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// The apps in the system tray (feature `tray`). Topic [`TOPIC`],
    /// sent right after subscribing and whenever one comes or goes.
    Tray(Tray),
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

/// The system tray (feature `tray`): the StatusNotifierItems apps
/// registered with novad's watcher, in the order they did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tray {
    pub items: Vec<TrayItem>,
    /// The apps the user hid from the bar's tray (`hide_tray_app`), by an
    /// item's app id, or its own `id` when it has no app. Quick settings
    /// still lists them, to bring them back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<String>,
}

/// One app's tray icon. The UI draws it through Quickshell's tray host and
/// finds it there by `id` and `title`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrayItem {
    /// Where it lives on the session bus: `<bus name>/<object path>`, as
    /// the watcher lists it.
    pub address: String,
    /// The item's own `Id`, like `discord_status_icon_1`.
    pub id: String,
    /// The item's own `Title`, empty for Electron apps.
    pub title: String,
    /// The installed app it belongs to, found from the process that owns
    /// it, when there is one (and the `launcher` feature is on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<App>,
    /// It has no `Activate` (libayatana's items, like Spotify's): a click
    /// on it shows its menu, as one on an item that says `ItemIsMenu` does.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub menu_only: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tray_event_is_on_its_topic() {
        assert_eq!(Event::Tray(Tray { items: vec![], hidden: vec![] }).topic(), Some(TOPIC));
    }
}
