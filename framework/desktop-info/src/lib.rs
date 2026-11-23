//! What the compositor shows, as the `compositor` event carries it to the
//! UI: the same [`Desktop`] whichever compositor it is. Plain data, so api
//! crates and fronts can name it without the adapters that build it
//! (`nova-compositor`).

use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};

/// The topic the desktop is published on.
pub const TOPIC: Topic = Topic::named("compositor");
/// The name of the event that carries it.
pub const EVENT: &str = "compositor";
/// Hyprland's setting that lets a lock screen that restarted take over the
/// session lock its crashed one held. Off unless the config turns it on,
/// which Nova's Hyprland rules do (`packaging/compositor/hyprland.lua`).
/// Named here so novad's check, `nova doctor` and the lock screen's log
/// agree.
pub const HYPRLAND_LOCK_RESTORE: &str = "misc:allow_session_lock_restore";

/// Pushed by novad on [`TOPIC`]. The compositor adapters publish it without
/// a copy (`nova_compositor::event`), the same on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// The compositor's workspaces, outputs, focused window and keyboard
    /// layout, sent right after subscribing and whenever any of it changes.
    /// Never sent when novad doesn't know the compositor.
    Compositor(Desktop),
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

/// What the compositor shows, the same shape whichever compositor it is.
/// The UI shows only what `capabilities` lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Desktop {
    /// The compositor's name, for people (`nova status`). The UI checks
    /// `capabilities`, never this.
    pub compositor: String,
    pub capabilities: Vec<Capability>,
    /// Sorted by name.
    pub outputs: Vec<Output>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_window: Option<FocusedWindow>,
    /// The active keyboard layout's name, like `English (US)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard_layout: Option<String>,
}

/// What a compositor can report or do. Something it can't is left out, and
/// the UI hides what depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// `outputs` lists each output's workspaces.
    Workspaces,
    /// `focus_workspace` and `step_workspace` work.
    FocusWorkspace,
    /// `focused_window` is reported.
    FocusedWindow,
    /// `keyboard_layout` is reported.
    KeyboardLayout,
    /// The power menu's log out works: it quits the compositor.
    LogOut,
    /// Workspaces report `fullscreen`.
    Fullscreen,
    /// Outputs list the compositor's special workspaces (`special`), and
    /// `focus_workspace` shows or hides one.
    SpecialWorkspaces,
}

/// A monitor, by its connector name (`DP-1`), as Quickshell's `screen.name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub name: String,
    /// Has the keyboard focus.
    pub focused: bool,
    /// In the compositor's order.
    pub workspaces: Vec<Workspace>,
    /// Goes up whenever windows on it move or change size, for the UI to
    /// notice (glass copies the screen again); the number itself means
    /// nothing. Hyprland reports no resizes.
    #[serde(default)]
    pub layout: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    /// What `focus_workspace` takes.
    pub id: WorkspaceId,
    /// Its name, or its number when it has none.
    pub name: String,
    /// Shown on its output.
    pub active: bool,
    /// Shown on the focused output.
    pub focused: bool,
    /// A window on it wants attention.
    pub urgent: bool,
    /// No windows on it.
    pub empty: bool,
    /// A window on it fills the whole output, over the bar's space: one in
    /// fullscreen (or niri's maximize to edges).
    pub fullscreen: bool,
    /// A special workspace (Hyprland's scratchpad): one that shows over an
    /// output's workspace and hides again. Every output lists them, first;
    /// it's `active` on the output showing it, and the workspace under it
    /// isn't.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub special: bool,
    /// A special workspace showing on another output.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shown_elsewhere: bool,
    /// The window focus goes to on it (`FocusedWindow::id`), known even
    /// while a Nova panel holds the keyboard and no window is focused. Only
    /// niri sends it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_window: Option<String>,
}

/// The compositor's own workspace id. Hyprland's named workspaces have
/// negative ids.
pub type WorkspaceId = i64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FocusedWindow {
    /// The compositor's own name for the window, to tell one from another
    /// (a title can change, and two windows can share an app id). Empty
    /// from a novad from before it was sent.
    #[serde(default)]
    pub id: String,
    /// The Wayland app id (Hyprland calls it the class), empty when unset.
    pub app_id: String,
    pub title: String,
}

/// Which way `step_workspace` goes in an output's workspace list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Next,
    Previous,
}
