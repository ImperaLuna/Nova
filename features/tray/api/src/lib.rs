//! The tray feature's messages: the apps in the system tray, for the bar's
//! tray and the quick settings tray card.
//!
//! A client sends a [`Command`] (`hide_tray_app`, `activate_tray_item`,
//! `tray_menu_picked` and the rest); novad publishes an [`Event`] on
//! [`TOPIC`] right after subscribing and whenever an item comes, goes or
//! changes its icon or status. The work is `nova-tray`
//! (`features/tray/core`).
//!
//! Today's Quickshell UI is a tray host of its own and reads each item's
//! icon and menu from the bus; the GPUI UI reads them here: an item's
//! [`TrayItem::icon`] and [`TrayItem::attention`], and its menu one level
//! at a time with `tray_menu` ([`TrayMenuLevel`]).

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
    /// A middle click on the tray item at `address` (feature `tray`): its
    /// `SecondaryActivate`.
    SecondaryActivateTrayItem { address: String },
    /// One level of the menu of the tray item at `address` (feature
    /// `tray`): the entries under entry `id`, 0 for the menu itself. The
    /// app is told the level is about to show first, so it can fill it.
    /// Answers with a [`TrayMenuLevel`].
    TrayMenu { address: String, id: i32 },
    /// Runs entry `id` of the menu of the tray item at `address` (feature
    /// `tray`), read at `revision` ([`TrayMenuLevel::revision`]). Notes the
    /// app's windows first, as `tray_menu_picked` does. A menu the app
    /// changed since is refused, so a click never lands on another entry.
    RunTrayMenuEntry { address: String, id: i32, revision: u32 },
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

/// One app's tray icon. Today's Quickshell UI draws it through its own
/// tray host and finds it there by `id` and `title`; the GPUI UI draws
/// [`TrayItem::icon`].
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
    /// The item's own picture as a file: its icon name looked up in its
    /// own folder (`IconThemePath`) or the icon theme, or the pixels it
    /// sent, written out as a PNG. Its attention picture while it asks for
    /// attention, when it has one. `None` when it has no usable picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// It asks for attention (`Status` is `NeedsAttention`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub attention: bool,
    /// It has a menu (`tray_menu` reads it).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_menu: bool,
    /// It says a click shows its menu rather than activating it
    /// (`ItemIsMenu`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_menu: bool,
}

/// One level of a tray app's menu, the answer to `tray_menu`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrayMenuLevel {
    /// The menu's layout revision these entries are from:
    /// `run_tray_menu_entry` passes it back.
    pub revision: u32,
    /// The entries that show, in order.
    pub entries: Vec<TrayMenuEntry>,
}

/// An entry of a tray app's menu, as its DBusMenu lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrayMenuEntry {
    pub id: i32,
    /// The app's label as it sends it, mnemonic markers (`_`) included.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// A line between groups of entries, with no label.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub separator: bool,
    #[serde(default = "enabled", skip_serializing_if = "is_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Toggle::is_none")]
    pub toggle: Toggle,
    /// Its checkbox or radio button is on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub checked: bool,
    /// It opens a submenu: `tray_menu` with its `id` reads that.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub submenu: bool,
}

fn enabled() -> bool {
    true
}

fn is_enabled(enabled: &bool) -> bool {
    *enabled
}

/// What a menu entry can be switched as.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Toggle {
    #[default]
    None,
    Checkmark,
    Radio,
}

impl Toggle {
    pub fn is_none(&self) -> bool {
        *self == Self::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tray_event_is_on_its_topic() {
        assert_eq!(Event::Tray(Tray { items: vec![], hidden: vec![] }).topic(), Some(TOPIC));
    }

    #[test]
    fn a_menu_entry_leaves_out_what_is_as_usual() {
        let entry = TrayMenuEntry {
            id: 3,
            label: "_Quit".into(),
            separator: false,
            enabled: true,
            toggle: Toggle::None,
            checked: false,
            submenu: false,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json, serde_json::json!({"id": 3, "label": "_Quit"}));
        assert_eq!(serde_json::from_value::<TrayMenuEntry>(json).unwrap(), entry);
    }
}
