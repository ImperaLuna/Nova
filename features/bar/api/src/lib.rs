//! `nova toggle`'s messages on novad's socket: the [`Panel`]s it opens or
//! closes, the command that asks, and the event that tells the UI. novad
//! answers the command itself, so it works with the bar off too.

use nova_carrier::{Published, Topic};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// The topic a client subscribes to for [`Event::Toggle`].
pub const TOPIC: Topic = Topic::named("toggle");

/// Sent by the UI or the `nova` CLI. Arguments with unknown fields are
/// refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Opens `panel` on the focused output, or closes it if it's open there
    /// (`nova toggle`). Fails when its feature isn't ready or no UI is
    /// connected to show it.
    Toggle { panel: Panel },
}

/// Pushed by novad on [`TOPIC`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// Open or close `panel` (`nova toggle`). `output` is the focused
    /// output's name, when novad knows it.
    Toggle {
        panel: Panel,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Some(TOPIC)
    }
}

/// A panel `nova toggle` opens or closes. Each belongs to the feature of the
/// same name and exists only while that feature is ready.
///
/// This is the one list of Nova's panels. It sits in the bar's api because
/// the stage the panels open on has no feature of its own, and the bar is
/// the feature they all open from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Panel {
    AiUsage,
    Power,
    QuickSettings,
    Launcher,
    /// The tray's icons in the right pill.
    Tray,
    /// The now-playing pill's panel, while a player shows.
    Media,
    /// The wallpaper carousel.
    Wallpaper,
}

impl Panel {
    pub const ALL: [Panel; 7] = [
        Panel::AiUsage,
        Panel::Power,
        Panel::QuickSettings,
        Panel::Launcher,
        Panel::Tray,
        Panel::Media,
        Panel::Wallpaper,
    ];

    /// Its name on the wire and on the command line, which is also its
    /// feature's id.
    pub fn name(self) -> &'static str {
        match self {
            Panel::AiUsage => "ai-usage",
            Panel::Power => "power",
            Panel::QuickSettings => "quick-settings",
            Panel::Launcher => "launcher",
            Panel::Tray => "tray",
            Panel::Media => "media",
            Panel::Wallpaper => "wallpaper",
        }
    }
}

impl fmt::Display for Panel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.name())
    }
}

impl FromStr for Panel {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::ALL.into_iter().find(|panel| panel.name() == text).ok_or_else(|| {
            let known: Vec<&str> = Self::ALL.iter().map(|panel| panel.name()).collect();
            format!("unknown panel {text:?} (known: {})", known.join(", "))
        })
    }
}
