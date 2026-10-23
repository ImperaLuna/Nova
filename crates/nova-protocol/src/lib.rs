//! Wire format between `novad` and its clients (the UI and the `nova` CLI).
//!
//! Every message is one JSON object per line on a Unix socket. The daemon
//! pushes [`Event`]s, clients send [`Request`]s, and the daemon answers each
//! request that carries an `id` with a [`Response`]. `hello` is sent on every
//! connection; other events only for the [`Topic`]s a client subscribed to.
//!
//! ```text
//! novad -> ui   {"v":2,"event":"hello","data":{"daemon":"0.1.0"}}
//! ui -> novad   {"v":2,"id":1,"cmd":"subscribe","args":{"topics":["features"]}}
//! novad -> ui   {"v":2,"id":1,"result":null}
//! novad -> ui   {"v":2,"event":"features","data":{"features":[...]}}
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Bumped on any incompatible change to the messages below.
pub const PROTOCOL_VERSION: u32 = 2;

/// Pushed by the daemon whenever its state changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// First message on every connection, whatever the subscriptions.
    Hello { daemon: String },
    /// Which features are usable right now. Topic [`Topic::Features`].
    Features { features: Vec<FeatureState> },
    /// Log entries from novad and the UI. Topic [`Topic::Logs`]. The first
    /// event after subscribing has `backlog: true` and holds recent entries.
    Logs {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        backlog: bool,
        entries: Vec<LogEntry>,
    },
    /// The active theme's colors. Topic [`Topic::Theme`], sent right after
    /// subscribing.
    Theme(Theme),
    /// Plan usage of AI coding tools. Topic [`Topic::AiUsage`], sent right
    /// after subscribing and whenever a number changes.
    AiUsage(AiUsage),
    /// The compositor's workspaces, outputs, focused window and keyboard
    /// layout. Topic [`Topic::Compositor`], sent right after subscribing and
    /// whenever any of it changes. Never sent when novad doesn't know the
    /// compositor.
    Compositor(Desktop),
    /// Open or close `panel` (`nova toggle`). Topic [`Topic::Toggle`].
    /// `output` is the focused output's name, when novad knows it.
    Toggle {
        panel: Panel,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
}

/// Groups of events a client can subscribe to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    Features,
    Logs,
    Theme,
    AiUsage,
    Compositor,
    Toggle,
}

impl Event {
    /// The topic a client must subscribe to for this event, `None` for events
    /// every client gets.
    pub fn topic(&self) -> Option<Topic> {
        match self {
            Event::Hello { .. } => None,
            Event::Features { .. } => Some(Topic::Features),
            Event::Logs { .. } => Some(Topic::Logs),
            Event::Theme(_) => Some(Topic::Theme),
            Event::AiUsage(_) => Some(Topic::AiUsage),
            Event::Compositor(_) => Some(Topic::Compositor),
            Event::Toggle { .. } => Some(Topic::Toggle),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureState {
    pub id: String,
    pub status: FeatureStatus,
    /// Binaries the feature needs but that aren't on `PATH`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    /// The theme file's name without `.toml`, e.g. `carbonfox`.
    pub id: String,
    /// Display name, e.g. `Carbonfox`.
    pub name: String,
    pub mode: ThemeMode,
    pub colors: Palette,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

/// Every color token a theme sets, by the name `ui/theme/Tokens.qml` exposes
/// it under. Theme files must set all of them and nothing else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Palette {
    pub bg: Color,
    pub bg_alt: Color,
    pub fg: Color,
    pub fg_alt: Color,
    pub fg_dim: Color,
    pub primary: Color,
    pub secondary: Color,
    pub on_accent: Color,
    pub info: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub swatch1: Color,
    pub swatch2: Color,
    pub swatch3: Color,
    pub swatch4: Color,
    pub swatch5: Color,
    pub swatch6: Color,
    pub swatch7: Color,
    pub swatch8: Color,
}

/// An opaque `#RRGGBB` color. Anything else is refused when parsed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Color([u8; 3]);

impl Color {
    pub fn rgb(&self) -> [u8; 3] {
        self.0
    }
}

impl FromStr for Color {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || format!("{text:?} isn't a #RRGGBB color");
        let hex = text.strip_prefix('#').filter(|hex| hex.len() == 6 && hex.is_ascii()).ok_or_else(invalid)?;
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).map_err(|_| invalid());
        Ok(Self([channel(0)?, channel(2)?, channel(4)?]))
    }
}

impl TryFrom<String> for Color {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<Color> for String {
    fn from(color: Color) -> Self {
        let [red, green, blue] = color.0;
        format!("#{red:02X}{green:02X}{blue:02X}")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiUsage {
    /// The provider the bar shows.
    pub shown: AiProvider,
    /// Every provider, in display order.
    pub providers: Vec<ProviderUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    Claude,
    Codex,
}

impl AiProvider {
    pub const ALL: [AiProvider; 2] = [AiProvider::Claude, AiProvider::Codex];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub provider: AiProvider,
    pub state: UsageState,
    /// The subscription's display name, like `Plus`, when the source says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// The windows the plan has right now, session first. Plans differ:
    /// some have only a weekly limit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limits: Vec<UsageLimit>,
    /// Why there are no numbers and how to get them, when unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageState {
    /// novad is still reading the source.
    Loading,
    Ready,
    /// No numbers; `hint` says why.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageLimit {
    pub window: UsageWindow,
    /// 0 to 100.
    pub percent: u8,
    /// When the window starts over, RFC 3339 UTC. Absent once it has, until
    /// the tool is used again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageWindow {
    /// The rolling 5-hour limit.
    Session,
    Weekly,
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
}

/// A monitor, by its connector name (`DP-1`), as Quickshell's `screen.name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub name: String,
    /// Has the keyboard focus.
    pub focused: bool,
    /// In the compositor's order.
    pub workspaces: Vec<Workspace>,
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
}

/// The compositor's own workspace id. Hyprland's named workspaces have
/// negative ids.
pub type WorkspaceId = i64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FocusedWindow {
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

/// A panel `nova toggle` opens or closes. Each belongs to the feature of the
/// same name and exists only while that feature is ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Panel {
    AiUsage,
}

impl Panel {
    pub const ALL: [Panel; 1] = [Panel::AiUsage];

    /// Its name on the wire and on the command line, which is also its
    /// feature's id.
    pub fn name(self) -> &'static str {
        match self {
            Panel::AiUsage => "ai-usage",
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

/// Sent by the UI or the `nova` CLI. Arguments with unknown fields are
/// refused, like unknown fields on the request line itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Ping,
    ListFeatures,
    /// Adds topics to this connection. Topics with state (features) get their
    /// current event right away; logs get a backlog.
    Subscribe {
        topics: Vec<Topic>,
    },
    /// Replies with a [`Status`].
    Status,
    /// Stops the UI and exits novad.
    Quit,
    /// Picks the provider the bar shows (feature `ai-usage`). Remembered
    /// across restarts.
    ShowAiUsage {
        provider: AiProvider,
    },
    /// Asks for fresh numbers now, when the panel opens (feature
    /// `ai-usage`). Asking again within a minute does nothing.
    RefreshAiUsage,
    /// Claude's limits, read from Claude Code's status line by
    /// `nova claude-statusline` (feature `ai-usage`).
    ReportClaudeUsage {
        limits: Vec<UsageLimit>,
    },
    /// Shows a workspace (capability `focus_workspace`), focusing its output.
    FocusWorkspace {
        id: WorkspaceId,
    },
    /// Shows the workspace after or before the active one on `output`
    /// (capability `focus_workspace`), like scrolling on the bar. Nothing
    /// happens at either end.
    StepWorkspace {
        output: String,
        step: Step,
    },
    /// Opens `panel` on the focused output, or closes it if it's open there
    /// (`nova toggle`). Fails when its feature isn't ready or no UI is
    /// connected to show it.
    Toggle {
        panel: Panel,
    },
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
    /// `quickshell --version` of the Quickshell novad runs, when it answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quickshell: Option<String>,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiState {
    Running,
    /// Exited; novad starts it again after a short wait.
    Restarting,
    /// novad was started with `--no-ui`.
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Crash {
    /// RFC 3339, UTC.
    pub time: String,
    /// The saved log lines leading up to it.
    pub log: String,
}

/// One log line. On disk it's [`LogEntry::to_line`]; on the wire it's JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    /// RFC 3339, UTC, millisecond precision.
    pub time: String,
    pub level: LogLevel,
    /// `ui` for the UI's output, else the novad module that wrote it.
    pub source: String,
    pub message: String,
}

/// Source tag of lines the UI printed.
pub const UI_LOG_SOURCE: &str = "ui";

impl LogEntry {
    /// `2026-10-22T13:45:09.123Z  WARN ui: message`, no newline.
    pub fn to_line(&self) -> String {
        format!("{} {:>5} {}: {}", self.time, self.level, self.source, self.message)
    }

    /// The inverse of [`LogEntry::to_line`]. `None` for lines in another format.
    pub fn parse_line(line: &str) -> Option<Self> {
        let (time, rest) = line.split_once(' ')?;
        let rest = rest.trim_start();
        let (level, rest) = rest.split_once(' ')?;
        let (source, message) = rest.split_once(": ")?;
        Some(Self { time: time.into(), level: level.parse().ok()?, source: source.into(), message: message.into() })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub const ALL: [LogLevel; 4] = [LogLevel::Debug, LogLevel::Info, LogLevel::Warn, LogLevel::Error];

    pub fn name(self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.name())
    }
}

impl FromStr for LogLevel {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|level| level.name().eq_ignore_ascii_case(text))
            .ok_or_else(|| format!("unknown log level {text:?} (known: debug, info, warn, error)"))
    }
}

/// A message with the protocol version, flattened around its body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub v: u32,
    #[serde(flatten)]
    pub body: T,
}

impl<T> Envelope<T> {
    pub fn new(body: T) -> Self {
        Self { v: PROTOCOL_VERSION, body }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Present when the sender wants a [`Response`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(flatten)]
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Result(serde_json::Value),
    Error(String),
}

/// Encodes a message as one line, newline included.
/// A request line exactly as it may arrive: no other fields. `Envelope` and
/// `Request` flatten their bodies, and serde can't reject unknown fields
/// through `flatten`, so incoming lines are checked against this first.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestLine {
    v: u32,
    #[serde(default)]
    id: Option<u64>,
    cmd: String,
    #[serde(default)]
    args: Option<serde_json::Value>,
}

/// Reads one request line from a client. Anything on the socket is
/// untrusted: unknown fields, unknown commands, bad arguments and other
/// protocol versions are all errors.
pub fn decode_request(line: &str) -> Result<Request, String> {
    let RequestLine { v, id, cmd, args } = serde_json::from_str(line).map_err(|e| e.to_string())?;
    if v != PROTOCOL_VERSION {
        return Err(format!("protocol version {v} isn't supported; this daemon speaks {PROTOCOL_VERSION}"));
    }
    let mut tagged = serde_json::Map::new();
    tagged.insert("cmd".into(), cmd.into());
    if let Some(args) = args {
        tagged.insert("args".into(), args);
    }
    let command = serde_json::from_value(tagged.into()).map_err(|e| e.to_string())?;
    Ok(Request { id, command })
}

pub fn encode<T: Serialize>(body: T) -> serde_json::Result<String> {
    let mut line = serde_json::to_string(&Envelope::new(body))?;
    line.push('\n');
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Every message shape on the wire. A change here changes the protocol:
    /// update `ui/daemon/protocol.js` too, then accept with `cargo insta review`.
    #[test]
    fn wire_format() {
        let lines = [
            encode(Event::Hello { daemon: "0.1.0".into() }),
            encode(Event::Features {
                features: vec![
                    FeatureState { id: "core".into(), status: FeatureStatus::Ready, missing: vec![] },
                    FeatureState {
                        id: "recording".into(),
                        status: FeatureStatus::MissingDeps,
                        missing: vec!["gpu-screen-recorder|wf-recorder".into()],
                    },
                    FeatureState { id: "weather".into(), status: FeatureStatus::Off, missing: vec![] },
                ],
            }),
            encode(Request { id: Some(7), command: Command::Ping }),
            encode(Request { id: None, command: Command::ListFeatures }),
            encode(Request {
                id: Some(9),
                command: Command::Subscribe {
                    topics: vec![
                        Topic::Features,
                        Topic::Logs,
                        Topic::Theme,
                        Topic::AiUsage,
                        Topic::Compositor,
                        Topic::Toggle,
                    ],
                },
            }),
            encode(Event::Theme(theme())),
            encode(Request { id: Some(10), command: Command::Status }),
            encode(Request { id: Some(11), command: Command::Quit }),
            encode(Event::Logs { backlog: true, entries: vec![log_entry()] }),
            encode(Event::Logs { backlog: false, entries: vec![log_entry()] }),
            encode(Response { id: 10, outcome: Outcome::Result(serde_json::to_value(status()).unwrap()) }),
            encode(Response { id: 7, outcome: Outcome::Result(json!("pong")) }),
            encode(Event::AiUsage(ai_usage())),
            encode(Request { id: Some(12), command: Command::ShowAiUsage { provider: AiProvider::Codex } }),
            encode(Request { id: None, command: Command::RefreshAiUsage }),
            encode(Request {
                id: Some(13),
                command: Command::ReportClaudeUsage { limits: ai_usage().providers[0].limits.clone() },
            }),
            encode(Event::Compositor(desktop())),
            encode(Request { id: Some(14), command: Command::FocusWorkspace { id: -1337 } }),
            encode(Request {
                id: None,
                command: Command::StepWorkspace { output: "DP-1".into(), step: Step::Previous },
            }),
            encode(Request { id: Some(15), command: Command::Toggle { panel: Panel::AiUsage } }),
            encode(Event::Toggle { panel: Panel::AiUsage, output: Some("DP-1".into()) }),
            encode(Event::Toggle { panel: Panel::AiUsage, output: None }),
            encode(Response { id: 8, outcome: Outcome::Error("bad request".into()) }),
        ];
        let all: String = lines.into_iter().map(Result::unwrap).collect();
        assert!(all.lines().all(|l| l.starts_with(r#"{"v":2,"#)));
        insta::assert_snapshot!(all);
    }

    /// Every wire name `ui/daemon/protocol.js` declares is one novad knows:
    /// commands and events as they appear in the wire format snapshot, the
    /// enums by parsing, feature ids by their folder in `features/`.
    #[test]
    fn protocol_js_names_are_known() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let js = std::fs::read_to_string(root.join("ui/daemon/protocol.js")).unwrap();
        let wire = include_str!("snapshots/nova_protocol__tests__wire_format.snap");
        let parses = |value: &str, kind: fn(serde_json::Value) -> bool| kind(json!(value));
        let mut checked = 0;
        for line in js.lines() {
            let Some((name, value)) = line.strip_prefix("var ").and_then(|line| line.split_once(" = \"")) else {
                continue;
            };
            let value = value.trim_end_matches("\";");
            let known = match name.split('_').next().unwrap() {
                "CMD" => wire.contains(&format!(r#""cmd":"{value}""#)),
                "EVENT" => wire.contains(&format!(r#""event":"{value}""#)),
                "TOPIC" => parses(value, |v| serde_json::from_value::<Topic>(v).is_ok()),
                "CAPABILITY" => parses(value, |v| serde_json::from_value::<Capability>(v).is_ok()),
                "STEP" => parses(value, |v| serde_json::from_value::<Step>(v).is_ok()),
                "PANEL" => parses(value, |v| serde_json::from_value::<Panel>(v).is_ok()),
                "FEATURE" => root.join("features").join(value).join("feature.toml").is_file(),
                _ => panic!("protocol.js: no check for {name}; add its kind to this test"),
            };
            assert!(known, "protocol.js: {name} = {value:?} isn't a name novad knows");
            checked += 1;
        }
        assert!(checked > 0, "protocol.js declares no names");
    }

    fn desktop() -> Desktop {
        let workspace =
            |id, name: &str, active, focused| Workspace { id, name: name.into(), active, focused, urgent: false };
        Desktop {
            compositor: "niri".into(),
            capabilities: vec![Capability::Workspaces, Capability::FocusedWindow],
            outputs: vec![
                Output {
                    name: "DP-1".into(),
                    focused: true,
                    workspaces: vec![workspace(1, "1", false, false), workspace(2, "web", true, true)],
                },
                Output {
                    name: "HDMI-A-1".into(),
                    focused: false,
                    workspaces: vec![Workspace { urgent: true, ..workspace(3, "1", true, false) }],
                },
            ],
            focused_window: Some(FocusedWindow { app_id: "foot".into(), title: "~".into() }),
            keyboard_layout: None,
        }
    }

    fn ai_usage() -> AiUsage {
        AiUsage {
            shown: AiProvider::Claude,
            providers: vec![
                ProviderUsage {
                    provider: AiProvider::Claude,
                    state: UsageState::Ready,
                    plan: None,
                    limits: vec![
                        UsageLimit {
                            window: UsageWindow::Session,
                            percent: 42,
                            resets_at: Some("2026-10-23T11:00:00Z".into()),
                        },
                        UsageLimit { window: UsageWindow::Weekly, percent: 61, resets_at: None },
                    ],
                    hint: None,
                },
                ProviderUsage {
                    provider: AiProvider::Codex,
                    state: UsageState::Unavailable,
                    plan: None,
                    limits: vec![],
                    hint: Some("No Codex sessions yet.".into()),
                },
            ],
        }
    }

    fn log_entry() -> LogEntry {
        LogEntry {
            time: "2026-10-22T13:45:09.123Z".into(),
            level: LogLevel::Warn,
            source: UI_LOG_SOURCE.into(),
            message: "Binding loop detected: x: y".into(),
        }
    }

    fn theme() -> Theme {
        let colors: Palette = serde_json::from_value(json!({
            "bg": "#1D1D1D", "bgAlt": "#393E46", "fg": "#F5E7C6", "fgAlt": "#525252", "fgDim": "#9A9A9A",
            "primary": "#FD7014", "secondary": "#BE95FF", "onAccent": "#1D1D1D",
            "info": "#00ADB5", "success": "#25BE6A", "warning": "#FAEB92", "error": "#EE5396",
            "swatch1": "#5988DB", "swatch2": "#E8650E", "swatch3": "#07A7A5", "swatch4": "#9067CC",
            "swatch5": "#029A43", "swatch6": "#049BE7", "swatch7": "#DD6098", "swatch8": "#9C7C02",
        }))
        .unwrap();
        Theme { id: "carbonfox".into(), name: "Carbonfox".into(), mode: ThemeMode::Dark, colors }
    }

    #[test]
    fn colors_are_six_digit_hex() {
        assert_eq!("#fd7014".parse::<Color>().unwrap().rgb(), [0xFD, 0x70, 0x14]);
        assert_eq!(String::from("#fd7014".parse::<Color>().unwrap()), "#FD7014");
        for bad in ["fd7014", "#fd701", "#fd70144", "#fd70zz", "#ffffffff", "#ééé"] {
            assert!(bad.parse::<Color>().is_err(), "{bad} parsed");
        }
    }

    fn status() -> Status {
        Status {
            daemon: "0.1.0".into(),
            pid: 100,
            uptime_secs: 42,
            pss_kib: Some(4096),
            quickshell: Some("0.3.2".into()),
            ui: UiStatus {
                state: UiState::Running,
                pid: Some(101),
                pss_kib: Some(180_000),
                restarts: 1,
                last_crash: Some(Crash {
                    time: "2026-10-22T13:40:00.000Z".into(),
                    log: "/home/me/.local/state/nova/crashes/2026-10-22T13-40-00.000Z.log".into(),
                }),
            },
            features: vec![FeatureState { id: "core".into(), status: FeatureStatus::Ready, missing: vec![] }],
            log_dir: "/home/me/.local/state/nova".into(),
            desktop: None,
        }
    }

    #[test]
    fn subscribe_parses_with_topics() {
        let req = decode_request(r#"{"v":2,"id":1,"cmd":"subscribe","args":{"topics":["features","logs"]}}"#).unwrap();
        assert_eq!(req.command, Command::Subscribe { topics: vec![Topic::Features, Topic::Logs] });
        assert!(decode_request(r#"{"v":2,"cmd":"subscribe","args":{"topics":["x"]}}"#).is_err());
    }

    #[test]
    fn panel_names_parse_and_match_the_wire() {
        for panel in Panel::ALL {
            assert_eq!(panel.name().parse::<Panel>(), Ok(panel));
            assert_eq!(serde_json::to_value(panel).unwrap(), json!(panel.name()));
        }
        assert!("launcher".parse::<Panel>().unwrap_err().contains("known: ai-usage"));
        let features = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../features");
        for panel in Panel::ALL {
            assert!(features.join(panel.name()).join("feature.toml").is_file(), "{panel} has no feature");
        }
    }

    #[test]
    fn log_lines_round_trip() {
        let entry = log_entry();
        let line = entry.to_line();
        assert_eq!(line, "2026-10-22T13:45:09.123Z  WARN ui: Binding loop detected: x: y");
        assert_eq!(LogEntry::parse_line(&line), Some(entry));
        assert_eq!(LogEntry::parse_line("not a log line"), None);
    }

    #[test]
    fn log_levels_parse_any_case_and_order_by_severity() {
        assert_eq!("WARN".parse::<LogLevel>(), Ok(LogLevel::Warn));
        assert_eq!("error".parse::<LogLevel>(), Ok(LogLevel::Error));
        assert!("loud".parse::<LogLevel>().is_err());
        assert!(LogLevel::Debug < LogLevel::Info && LogLevel::Warn < LogLevel::Error);
    }

    #[test]
    fn only_hello_has_no_topic() {
        assert_eq!(Event::Hello { daemon: "x".into() }.topic(), None);
        assert_eq!(Event::Features { features: vec![] }.topic(), Some(Topic::Features));
        assert_eq!(Event::Logs { backlog: false, entries: vec![] }.topic(), Some(Topic::Logs));
        assert_eq!(Event::Theme(theme()).topic(), Some(Topic::Theme));
        assert_eq!(Event::AiUsage(ai_usage()).topic(), Some(Topic::AiUsage));
        assert_eq!(Event::Compositor(desktop()).topic(), Some(Topic::Compositor));
        assert_eq!(Event::Toggle { panel: Panel::AiUsage, output: None }.topic(), Some(Topic::Toggle));
    }

    #[test]
    fn unknown_argument_fields_are_rejected() {
        let error = decode_request(r#"{"v":2,"cmd":"subscribe","args":{"topics":["logs"],"unexpected":true}}"#);
        assert!(error.unwrap_err().contains("unknown field `unexpected`"));
        assert!(decode_request(r#"{"v":2,"cmd":"ping","args":{"unexpected":true}}"#).is_err());
        let limit = r#"{"window":"weekly","percent":5,"extra":1}"#;
        assert!(
            decode_request(&format!(r#"{{"v":2,"cmd":"report_claude_usage","args":{{"limits":[{limit}]}}}}"#)).is_err()
        );
    }

    #[test]
    fn request_without_args_parses() {
        let req = decode_request(r#"{"v":2,"id":7,"cmd":"ping"}"#).unwrap();
        assert_eq!(req, Request { id: Some(7), command: Command::Ping });
    }

    #[test]
    fn request_without_id_parses() {
        let req = decode_request(r#"{"v":2,"cmd":"list_features"}"#).unwrap();
        assert_eq!(req.id, None);
        assert_eq!(req.command, Command::ListFeatures);
    }

    #[test]
    fn unknown_command_is_rejected() {
        assert!(decode_request(r#"{"v":2,"cmd":"reboot"}"#).is_err());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(decode_request(r#"{"v":2,"id":1,"cmd":"ping"}"#).is_ok());
        assert!(decode_request(r#"{"v":2,"id":1,"cmd":"ping","extra":true}"#).is_err());
        assert!(decode_request(r#"{"v":2,"cmd":"ping","args":{"x":1}}"#).is_err());
    }

    #[test]
    fn other_protocol_versions_are_rejected() {
        let error = decode_request(r#"{"v":1,"id":1,"cmd":"ping"}"#).unwrap_err();
        assert!(error.contains("version 1"), "{error}");
        assert!(decode_request(r#"{"id":1,"cmd":"ping"}"#).is_err());
    }

    #[test]
    fn responses_round_trip() {
        for outcome in [Outcome::Result(json!("pong")), Outcome::Error("nope".into())] {
            let msg = Envelope::new(Response { id: 3, outcome });
            let back: Envelope<Response> = serde_json::from_str(&serde_json::to_string(&msg).unwrap()).unwrap();
            assert_eq!(back, msg);
        }
    }
}
