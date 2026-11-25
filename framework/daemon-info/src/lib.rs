//! novad's own messages on the socket, which no feature owns: the hello
//! every connection gets, the feature list, the log feed, `status` and the
//! commands that run the connection itself. Plain data, so fronts can name
//! them without novad.
//!
//! Every message is one JSON object per line on a Unix socket. The daemon
//! pushes events, clients send requests, and the daemon answers each
//! request that carries an `id` with a response (`nova-carrier`). `hello`
//! is sent on every connection; other events only for the topics a client
//! subscribed to. Each feature's messages are in its api crate.
//!
//! ```text
//! novad -> ui   {"v":2,"event":"hello","data":{"daemon":"0.1.0"}}
//! ui -> novad   {"v":2,"id":1,"cmd":"subscribe","args":{"topics":["features"]}}
//! novad -> ui   {"v":2,"id":1,"result":null}
//! novad -> ui   {"v":2,"event":"features","data":{"features":[...]}}
//! ```

use nova_carrier::{Published, Topic};
use nova_desktop_info::Desktop;
use serde::{Deserialize, Serialize};

/// The environment variable novad gives the UI its socket's path in.
pub const SOCKET_VAR: &str = "NOVA_SOCKET";

/// novad's own events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// First message on every connection, whatever the subscriptions.
    Hello { daemon: String },
    /// Which features are usable right now. Topic [`topics::FEATURES`].
    Features { features: Vec<FeatureState> },
    /// Log entries from novad and the UI. Topic [`topics::LOGS`]. The first
    /// event after subscribing has `backlog: true` and holds recent entries.
    Logs(nova_carrier::log::Logs),
}

/// The topics of the events in [`Event`].
pub mod topics {
    use nova_carrier::Topic;

    pub const FEATURES: Topic = Topic::named("features");
    /// The framework's log feed (`nova_carrier::log`).
    pub const LOGS: Topic = nova_carrier::log::TOPIC;
}

impl Event {
    /// The topic a client must subscribe to for this event, `None` for
    /// `hello`, which every client gets.
    pub fn topic(&self) -> Option<Topic> {
        match self {
            Event::Hello { .. } => None,
            Event::Features { .. } => Some(topics::FEATURES),
            Event::Logs(_) => Some(topics::LOGS),
        }
    }
}

impl Published for Event {
    fn topic(&self) -> Option<Topic> {
        Event::topic(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureState {
    pub id: String,
    pub status: FeatureStatus,
    /// Binaries the feature needs but that aren't on `PATH`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    /// Where its UI goes (its manifest's `[[slot]]` entries). The UI fills
    /// each host with the slots of the features that are ready.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<Slot>,
    /// The mode it runs in, for a feature with modes that's on (its
    /// manifest's `modes`, picked in `features.toml`'s `[modes]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// One place a feature's UI fills (`[[slot]]` in `feature.toml`): a view in
/// a host, like the now-playing pill in the bar's left row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    /// The host's name, like `bar.left` (`nova_features::HOSTS`).
    pub host: String,
    /// The view's id, like `media.pill`: the feature's UI registers a view
    /// under it, so the manifest never names a file.
    pub view: String,
    /// Sort key inside the host, lowest first (leftmost in a row), counted
    /// in tens to leave room. Ties sort by feature id.
    #[serde(default)]
    pub order: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureStatus {
    /// Turned on and all dependencies are present. The UI loads it.
    Ready,
    /// Turned on but a dependency is missing. The UI doesn't load it.
    MissingDeps,
    /// Turned off. The UI doesn't load it.
    Off,
}

/// novad's own commands, sent by the UI or the `nova` CLI. Arguments with
/// unknown fields are refused, like unknown fields on the request line
/// itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Ping,
    ListFeatures,
    /// Adds topics to this connection, by their wire names ([`Topic::name`]).
    /// Topics with state (features) get their current event right away;
    /// logs get a backlog. novad refuses a name that isn't one of its
    /// topics, like an unknown variant.
    Subscribe {
        topics: Vec<String>,
    },
    /// Replies with a [`Status`].
    Status,
    /// Stops the UI and exits novad.
    Quit,
}

/// What `status` returns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub daemon: String,
    pub pid: u32,
    pub uptime_secs: u64,
    /// Proportional memory use in KiB, when readable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pss_kib: Option<u64>,
    pub ui: UiStatus,
    pub features: Vec<FeatureState>,
    /// The folder novad writes its log and crash snapshots to.
    pub log_dir: String,
    /// What the compositor shows now, when novad knows the compositor and
    /// has heard from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop: Option<Desktop>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiStatus {
    pub state: UiState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pss_kib: Option<u64>,
    /// Times the UI was restarted after exiting on its own.
    pub restarts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_crash: Option<Crash>,
    /// When the UI got to its state: when it started, while it runs, else
    /// since when it hasn't run (RFC 3339, UTC). Missing from a novad that
    /// hasn't tried to run it yet, or from before IMP-467.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// Why the UI isn't running while novad wants it: the last start's
    /// error, or how it last exited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiState {
    Running,
    /// Exited; novad starts it again after a short wait.
    Restarting,
    /// novad was started with `--no-ui`.
    Disabled,
    /// No feature with a UI is on, so novad doesn't run the UI.
    NotNeeded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Crash {
    /// RFC 3339, UTC.
    pub time: String,
    /// The saved log lines leading up to it.
    pub log: String,
}
