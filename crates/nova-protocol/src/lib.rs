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

#[cfg(test)]
mod js;

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
    /// Whether the session is locked (feature `lock`). Topic [`Topic::Lock`],
    /// sent right after subscribing and whenever it changes. The UI locks
    /// when `locked` turns true, but only ever unlocks itself, after the
    /// user's password checks out: `locked: false` just follows that.
    Lock { locked: bool },
    /// Change the default output's volume as `nova volume` asks, and show
    /// the volume capsule. Topic [`Topic::Volume`].
    Volume { key: VolumeKey },
    /// The screens are frozen for a region screenshot (feature `screenshot`):
    /// each frame is what one output showed, for the UI to draw under the
    /// region picker. Topic [`Topic::Screenshot`].
    Capture { frames: Vec<CaptureFrame> },
    /// The region picker closes: the shot was taken or cancelled. Topic
    /// [`Topic::Screenshot`].
    CaptureEnded,
    /// A screenshot was saved, and copied when it could be (feature
    /// `screenshot`). `id` names it in `screenshot_action`. Topic
    /// [`Topic::Screenshot`].
    ScreenshotSaved(SavedScreenshot),
    /// `nova record` while nothing is being recorded (feature `recording`):
    /// the UI opens the region picker to pick what to record, or closes it
    /// when it's open. Topic [`Topic::Recording`].
    PickRecording,
    /// What's being recorded, or `null` once nothing is (feature
    /// `recording`). Topic [`Topic::Recording`], sent right after subscribing
    /// and whenever it starts, stops or pauses.
    Recording(Option<ActiveRecording>),
    /// A recording was saved (feature `recording`). `id` names it in
    /// `recording_action`. Topic [`Topic::Recording`].
    RecordingSaved(SavedRecording),
    /// The machine and its user, for the quick settings header. Topic
    /// [`Topic::Host`], sent right after subscribing.
    Host(Host),
    /// What the machine is busy with (feature `resources`). Topic
    /// [`Topic::Resources`], sent every few seconds while some client
    /// watches them (`watch_resources`), and right when one starts to.
    Resources(Resources),
    /// The apps the launcher can open, and the user's pinned and recent ones
    /// (feature `launcher`). Topic [`Topic::Launcher`], sent right after
    /// subscribing and whenever any of it changes.
    Launcher(Launcher),
    /// The apps in the system tray (feature `tray`). Topic [`Topic::Tray`],
    /// sent right after subscribing and whenever one comes or goes.
    Tray(Tray),
    /// The notifications kept for the quick settings list, do not disturb
    /// and the muted apps (feature `notifications`). Topic
    /// [`Topic::Notifications`], sent right after subscribing and whenever
    /// any of it changes.
    Notifications(NotificationList),
    /// A notification to pop up: a new one, or one an app replaced while
    /// it may still show (feature `notifications`). Never for a muted app,
    /// nor during do not disturb unless it's critical. Topic
    /// [`Topic::Notifications`].
    NotificationPopup(Notification),
    /// The media players on the session bus and the one the bar shows
    /// (feature `media`). Topic [`Topic::Media`], sent right after
    /// subscribing and whenever a player comes, goes or changes.
    Media(Media),
    /// Wireless devices and their batteries (feature `peripherals`). Topic
    /// [`Topic::Peripherals`], sent right after subscribing and whenever a
    /// device comes, goes or changes.
    Peripherals(Peripherals),
    /// Wired and Wi-Fi links from NetworkManager (feature `network`). Topic
    /// [`Topic::Network`], sent right after subscribing and whenever a link,
    /// a network in reach or a connection attempt changes.
    Network(Network),
    /// The traffic over the machine's network links, every
    /// [`NETWORK_TRAFFIC_INTERVAL_SECS`] while some connection watches it
    /// (`watch_network_traffic`, feature `network`). Topic [`Topic::Network`].
    NetworkTraffic(NetworkTraffic),
    /// Bluetooth: the radio, the devices and the pairing waiting for the
    /// user (feature `bluetooth`). Topic [`Topic::Bluetooth`], sent right
    /// after subscribing and whenever any of it changes.
    Bluetooth(Bluetooth),
    /// A Nova Files window to open (feature `files`), as `nova files` or an
    /// app's "Show in folder" asked. Sent only to the Nova Files process,
    /// once it has attached (`attach_files`); one asked for before then is
    /// sent right after. Topic [`Topic::Files`].
    FilesWindow(FilesWindow),
    /// One page of a folder's entries in view `view` (feature `files`), in
    /// the view's order: entries `start..start + entries.len()` of `total`.
    /// A page with `start: 0` replaces everything the view held, after
    /// `files_open` and again when novad had to read the folder anew (it
    /// lost track of changes). Sent only to the client that opened the
    /// view, whatever it subscribed to; topic [`Topic::Files`].
    FilesPage(FolderPage),
    /// The folder in view `view` changed: apply `changes` in order, each
    /// index counting in the list as the changes before it left it. Sent
    /// only to the client that opened the view; topic [`Topic::Files`].
    FilesChanged { view: u32, changes: Vec<FolderChange> },
    /// novad closed view `view` (feature `files`) by itself: its folder
    /// couldn't be read, or was deleted, moved away or unmounted. `error`
    /// says why, for the window. Sent only to the client that opened the
    /// view; topic [`Topic::Files`].
    FilesClosed { view: u32, error: String },
    /// How solid Nova Files' windows are (feature `files`), from
    /// `config.toml`'s `[files]`. Topic [`Topic::Files`], sent right after
    /// subscribing and whenever it changes.
    FilesLook(FilesLook),
    /// Something in the trash changed, in Nova Files or any other app
    /// (feature `files`): ask `files_trash_list` again. Topic
    /// [`Topic::Files`].
    FilesTrashChanged,
    /// The drives Nova Files' sidebar lists (feature `files`): every
    /// filesystem udisks2 knows that a person would open, as it is now.
    /// Topic [`Topic::Files`], sent right after subscribing and whenever a
    /// drive comes, goes, mounts, unmounts or fills up.
    FilesDrives { drives: Vec<Drive> },
    /// How copy or move job `job` is going (feature `files`): as it
    /// starts, every [`FILES_TRANSFER_PROGRESS_MS`] while it runs, when it
    /// asks about a name clash and when it ends. Sent only to the client
    /// that started the job; topic [`Topic::Files`].
    FilesTransfer(Transfer),
    /// How far measuring job `job` got (feature `files`, `files_measure`):
    /// every [`FILES_MEASURE_PROGRESS_MS`] while it counts, and once more
    /// with `done` when it's finished. Sent only to the client that started
    /// the job; topic [`Topic::Files`].
    FilesMeasured(Measured),
    /// What Ctrl+Z and Ctrl+Y would do now in any Nova Files window
    /// (feature `files`): the kind of step each would undo or redo, none
    /// when there's nothing. novad keeps one history for the session.
    /// Topic [`Topic::Files`], sent right after subscribing and whenever
    /// either changes.
    FilesHistory {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        undo: Option<FileStep>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        redo: Option<FileStep>,
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
    Lock,
    Volume,
    Screenshot,
    Recording,
    Host,
    Resources,
    Launcher,
    Tray,
    Notifications,
    Media,
    Peripherals,
    Network,
    Bluetooth,
    /// Nova Files' own (feature `files`), for the Nova Files process.
    Files,
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
            Event::Lock { .. } => Some(Topic::Lock),
            Event::Volume { .. } => Some(Topic::Volume),
            Event::Capture { .. } | Event::CaptureEnded | Event::ScreenshotSaved(_) => Some(Topic::Screenshot),
            Event::PickRecording | Event::Recording(_) | Event::RecordingSaved(_) => Some(Topic::Recording),
            Event::Host(_) => Some(Topic::Host),
            Event::Resources(_) => Some(Topic::Resources),
            Event::Launcher(_) => Some(Topic::Launcher),
            Event::Tray(_) => Some(Topic::Tray),
            Event::Notifications(_) | Event::NotificationPopup(_) => Some(Topic::Notifications),
            Event::Media(_) => Some(Topic::Media),
            Event::Peripherals(_) => Some(Topic::Peripherals),
            Event::Network(_) | Event::NetworkTraffic(_) => Some(Topic::Network),
            Event::Bluetooth(_) => Some(Topic::Bluetooth),
            Event::FilesWindow(_) => Some(Topic::Files),
            Event::FilesPage(_)
            | Event::FilesChanged { .. }
            | Event::FilesClosed { .. }
            | Event::FilesTrashChanged
            | Event::FilesTransfer(_)
            | Event::FilesMeasured(_)
            | Event::FilesDrives { .. }
            | Event::FilesHistory { .. } => Some(Topic::Files),
            Event::FilesLook(_) => Some(Topic::Files),
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
    /// Where its UI goes (its manifest's `[[slot]]` entries). The UI fills
    /// each host with the slots of the features that are ready.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<Slot>,
}

/// One place a feature's UI fills (`[[slot]]` in `feature.toml`): a view in
/// a host, like the now-playing pill in the bar's left row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    /// The host's name, like `bar.left` (`nova_features::HOSTS`).
    pub host: String,
    /// The view's id, like `media.pill`: the UI maps it to its component
    /// (ui/slots/Views.qml), so the manifest never names a file.
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
    /// Behind everything when nothing else is drawn, like the lock screen.
    pub backdrop: Color,
    /// Glass: laid over the compositor's blur, translucent (Tokens sets how
    /// much).
    pub glass_tint: Color,
    /// Text and icons. Tokens derives the dimmer levels from it.
    pub fg: Color,
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

/// A panel `nova toggle` opens or closes. Each belongs to the feature of the
/// same name and exists only while that feature is ready.
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

/// The devices with a battery (feature `peripherals`): mice, keyboards,
/// headsets and the like that are connected and on right now and report
/// one, each listed once however many sources report it. One that's off or
/// out of reach (a headset whose dongle stays plugged in) isn't listed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peripherals {
    pub devices: Vec<Peripheral>,
}

/// One device and its battery. Text is plain and capped in length.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peripheral {
    /// Stable while the device stays, like `upower/mouse_hidpp_battery_0`
    /// or `headsetcontrol/1038:2253`.
    pub id: String,
    /// Its model, like `PRO X Wireless`.
    pub name: String,
    pub kind: PeripheralKind,
    /// How it's connected, when novad can tell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<PeripheralLink>,
    /// Its charge, when it has said yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery: Option<Battery>,
    pub charging: bool,
    /// Where the reading comes from.
    pub source: PeripheralSource,
}

/// What a device is, for its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeripheralKind {
    Mouse,
    Keyboard,
    Touchpad,
    GamingInput,
    Pen,
    Tablet,
    Headset,
    Headphones,
    Speakers,
    RemoteControl,
    Wearable,
    Other,
}

/// How a device is connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeripheralLink {
    /// Through its own USB receiver (2.4 GHz).
    Dongle,
    Bluetooth,
    /// By its cable.
    Wired,
}

/// A device's charge: a percentage, or only a rough level for devices that
/// report no more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Battery {
    /// 0 to 100.
    Percent(u8),
    Level(BatteryLevel),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatteryLevel {
    Critical,
    Low,
    Normal,
    High,
    Full,
}

/// Where novad read a device: UPower (the kernel's drivers and BlueZ),
/// HID++ (Logitech devices on a receiver the kernel doesn't drive),
/// Keychron (keyboards on firmware that reports the battery over raw HID)
/// or HeadsetControl (headsets the kernel doesn't know the battery of).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeripheralSource {
    Upower,
    Hidpp,
    Keychron,
    Headsetcontrol,
}

/// The machine's network links as NetworkManager has them (feature
/// `network`). While NetworkManager doesn't run, `running` is false and the
/// rest is empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub running: bool,
    /// The kind of link traffic goes over now (NetworkManager's primary
    /// connection), `None` while offline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<LinkKind>,
    /// Which link should carry traffic while both a cable and Wi-Fi are
    /// connected: the user's pick (`prefer_network`), else, while both are
    /// connected, the one that does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefer: Option<LinkKind>,
    /// Every Ethernet port NetworkManager manages.
    pub wired: Vec<WiredLink>,
    /// The first Wi-Fi card NetworkManager manages, `None` without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wifi: Option<Wifi>,
}

/// What a network link is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    Wired,
    Wifi,
    /// Anything else NetworkManager connects through (a phone over USB, a
    /// mobile broadband modem, a VPN it routes everything over).
    Other,
}

/// Where a link is in connecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    /// It can't connect now: no cable in the port, or Wi-Fi turned off.
    Unavailable,
    Disconnected,
    Connecting,
    Connected,
}

/// One Ethernet port.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WiredLink {
    /// The kernel's name for it, like `enp5s0`.
    pub interface: String,
    pub state: LinkState,
    /// The connection profile in use, like `Wired connection 1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The link speed the cable negotiated, while connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_mbps: Option<u32>,
}

/// The Wi-Fi card and the networks in its reach.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wifi {
    pub interface: String,
    /// Wi-Fi's switch (`set_wifi`).
    pub enabled: bool,
    /// Turned off by a hardware switch or an airplane mode key: `set_wifi`
    /// can't turn it on.
    pub blocked: bool,
    pub state: LinkState,
    /// The network it's on, or connecting to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    /// The speed it's talking to the access point at, while connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_mbps: Option<u32>,
    /// A scan was asked for (`scan_wifi`) and hasn't finished.
    pub scanning: bool,
    /// The networks in reach, each name once, the one it's on first, then
    /// the strongest. Networks that hide their name aren't listed.
    pub networks: Vec<WifiNetwork>,
    /// How the last `connect_wifi` went, until the next one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<WifiAttempt>,
}

/// A Wi-Fi network in reach, from its strongest access point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WifiNetwork {
    /// Its name (SSID), as text: bytes that aren't UTF-8 are replaced.
    pub ssid: String,
    /// Signal strength, 0 to 100.
    pub strength: u8,
    pub security: WifiSecurity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band: Option<WifiBand>,
    /// NetworkManager has a profile for it, so it connects without asking
    /// for a password.
    pub saved: bool,
    /// The card is on it now.
    pub connected: bool,
}

/// How a Wi-Fi network is secured, its best method when it offers several.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiSecurity {
    /// No password (including Enhanced Open, which encrypts without one).
    Open,
    /// The old WEP: Nova doesn't connect to these.
    Wep,
    Wpa,
    Wpa2,
    Wpa3,
    /// 802.1X (work and school networks, like eduroam): set up in the
    /// system's network settings, Nova doesn't connect to these.
    Enterprise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiBand {
    #[serde(rename = "2.4ghz")]
    Ghz2_4,
    #[serde(rename = "5ghz")]
    Ghz5,
    #[serde(rename = "6ghz")]
    Ghz6,
}

/// How a `connect_wifi` is going.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WifiAttempt {
    pub ssid: String,
    pub outcome: AttemptOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptOutcome {
    Connecting,
    Connected,
    /// The network turned the password down, or none was given for a
    /// network that needs one: ask for it.
    WrongPassword,
    /// It failed some other way (out of reach, no address).
    Failed,
}

/// How often traffic is sampled while watched, and the step of its history.
pub const NETWORK_TRAFFIC_INTERVAL_SECS: u64 = 1;
/// How far back a traffic history goes.
pub const NETWORK_TRAFFIC_HISTORY_SECS: u64 = 60;

/// Bytes over the machine's wired and Wi-Fi links together (not over
/// loopback, bridges or tunnels, whose traffic also crosses one of those).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkTraffic {
    /// Bytes per second down and up, one per
    /// [`NETWORK_TRAFFIC_INTERVAL_SECS`], oldest first, up to
    /// [`NETWORK_TRAFFIC_HISTORY_SECS`] back; the last is now.
    pub down: Vec<u64>,
    pub up: Vec<u64>,
    /// Bytes since local midnight, counted while novad ran.
    pub today_down: u64,
    pub today_up: u64,
}

/// A Wi-Fi password, on its way from the UI to NetworkManager. It never
/// shows in a log: its `Debug` hides it, and it has no `Display`. It's
/// serialized only to send it (the `nova` CLI, tests).
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct WifiPassword(String);

impl WifiPassword {
    pub fn new(password: impl Into<String>) -> Self {
        Self(password.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for WifiPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WifiPassword(<redacted>)")
    }
}

impl Serialize for WifiPassword {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

/// The longest network name a command takes, in bytes (802.11's limit).
pub const WIFI_SSID_MAX_BYTES: usize = 32;
/// The longest password a command takes, in bytes: WPA's passphrases stop
/// at 63 (64 as hex), WPA3's have no limit of their own.
pub const WIFI_PASSWORD_MAX_BYTES: usize = 128;
/// The shortest WPA passphrase, in bytes (WPA3's SAE takes shorter ones, but
/// a shorter one is never a WPA2 key, and networks offering both are common).
pub const WPA_PASSWORD_MIN_BYTES: usize = 8;

/// A Nova Files window to open (feature `files`): the folder it shows, an
/// absolute path, and the items in it to select, by absolute path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesWindow {
    pub folder: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub select: Vec<String>,
}

/// How solid Nova Files' window frame (the rows above the files and the
/// sidebar) and its file list are, each a percentage from glass (0) to the
/// theme's solid surfaces (100).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesLook {
    pub frame: u8,
    pub list: u8,
}

/// The most solid a [`FilesLook`] part can be: the theme's solid surface.
pub const FILES_SOLID_MAX: u8 = 100;

/// The apps that can open a file (feature `files`), for its "Open with"
/// menu: the answer to [`Command::FilesOpenWith`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenWith {
    /// The file's type, like `image/png`.
    pub mime: String,
    /// The id of the app that opens it by default, when one does: the
    /// first of `apps`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// The apps that can open it, the default first.
    pub apps: Vec<App>,
}

/// Bluetooth as the quick settings page shows it (feature `bluetooth`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bluetooth {
    pub radio: BluetoothRadio,
    /// Looking for new devices now (`watch_bluetooth`).
    pub scanning: bool,
    /// Connected devices first, then paired ones, then the new ones nearby,
    /// each group by name. Only while the radio is on.
    pub devices: Vec<BluetoothDevice>,
    /// What pairing asks of the user now, if anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pairing: Option<PairingRequest>,
}

/// Whether Bluetooth can be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BluetoothRadio {
    /// No adapter, or BlueZ isn't running.
    Missing,
    /// Off, or blocked in software: `bluetooth_power` turns it on either way.
    Off,
    /// Off by a hardware switch or key, which only the user can turn on.
    SwitchedOff,
    On,
}

/// A device the adapter knows. Text is plain and capped in length.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BluetoothDevice {
    /// Its address, like `9C:2A:70:11:22:33`: the id the commands take.
    pub address: String,
    pub name: String,
    pub kind: BluetoothKind,
    pub status: BluetoothStatus,
    /// 0 to 100, when the device reports its battery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery: Option<u8>,
    /// Why the last connect, pair or disconnect failed, in words for the
    /// user. Cleared by the next one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What a Bluetooth device is, for its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BluetoothKind {
    Headphones,
    Headset,
    Speaker,
    Keyboard,
    Mouse,
    Gamepad,
    Tablet,
    Phone,
    Computer,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BluetoothStatus {
    /// Seen while scanning, never paired.
    Nearby,
    Pairing,
    /// Paired and not connected.
    Paired,
    Connecting,
    Connected,
    Disconnecting,
}

/// A question pairing asks the user, answered with `bluetooth_answer`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairingRequest {
    /// Names it in `bluetooth_answer`.
    pub id: u64,
    /// The device's address; it's in `devices`.
    pub address: String,
    pub ask: PairingAsk,
    /// The device started it (a phone pairing with this computer), not
    /// `bluetooth_connect`.
    pub incoming: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PairingAsk {
    /// Does the device show this code? `accept` or `reject`.
    Confirm { code: String },
    /// Type the PIN the device came with (old devices): answer a `code`.
    EnterPin,
    /// Type the six-digit code the device shows: answer a `code`.
    EnterPasskey,
    /// Type this PIN on the device. Needs no answer.
    ShowPin { pin: String },
    /// Type this code on the device; `typed` digits are in so far. Needs no
    /// answer.
    ShowPasskey { code: String, typed: u8 },
    /// May this device pair (or use a service)? `accept` or `reject`.
    Authorize,
}

/// The user's answer to a [`PairingRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PairingAnswer {
    Accept,
    Reject,
    /// The PIN or passkey typed.
    Code(String),
}

/// The longest PIN a `bluetooth_answer` takes, in bytes (BlueZ's limit).
pub const BLUETOOTH_PIN_MAX_BYTES: usize = 16;

/// The longest player id a media command takes, in bytes: a bus name is at
/// most 255.
pub const MEDIA_PLAYER_MAX_BYTES: usize = 255;

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

/// The longest `launcher_search` query, in characters: anything on the
/// socket is capped, and nobody types more.
pub const LAUNCHER_SEARCH_MAX_CHARS: usize = 200;

/// Most folder views one connection may have open (`files_open`): each
/// holds its folder's entries in novad.
pub const FILES_MAX_VIEWS: usize = 16;

/// The longest `files_open` path, in bytes: Linux's own limit.
pub const FILES_PATH_MAX_BYTES: usize = 4096;

/// The longest `files_search` query, in characters: anything on the socket
/// is capped, and nobody types more.
pub const FILES_SEARCH_MAX_CHARS: usize = 200;

/// Most results a `files_search` answers with: more than a list shows
/// before anyone types another letter.
pub const FILES_SEARCH_MAX_RESULTS: usize = 200;

/// Most folders a `files_tree` may name as `opened` or as `closed`, each.
pub const FILES_TREE_MAX_FOLDERS: usize = 256;
/// The longest request line novad reads: a longer one closes the
/// connection, so a client splits a long list across requests.
pub const REQUEST_MAX_BYTES: u64 = 64 * 1024;
/// Most copy or move jobs one connection may run at once
/// (`files_transfer`).
pub const FILES_MAX_TRANSFERS: usize = 16;

/// Most items one `files_transfer` takes: a whole folder's selection, with
/// each path at most [`FILES_PATH_MAX_BYTES`].
pub const FILES_TRANSFER_MAX_SOURCES: usize = 10_000;

/// Most measuring jobs one connection may run at once (`files_measure`).
pub const FILES_MAX_MEASURES: usize = 16;

/// How often a running `files_measure` job reports its counts, in
/// milliseconds.
pub const FILES_MEASURE_PROGRESS_MS: u64 = 150;

/// How often a running copy or move reports its progress, in milliseconds.
pub const FILES_TRANSFER_PROGRESS_MS: u64 = 100;

/// The longest wire text a path of [`FILES_PATH_MAX_BYTES`] can be: each
/// byte is at most a three-byte character ([`path_to_wire`]).
pub const FILES_PATH_MAX_WIRE_BYTES: usize = 3 * FILES_PATH_MAX_BYTES;

/// Where the characters that stand for raw bytes start (feature `files`,
/// [`path_to_wire`]): byte `b` (0x80 to 0xFF) is `RAW_BYTE_BASE + b`, in
/// the Private Use Area.
pub const RAW_BYTE_BASE: u32 = 0xF700;

/// A file name or path as wire text, losslessly (feature `files`). Linux
/// names are bytes and needn't be UTF-8 (old archives, Windows drives), but
/// JSON strings must be. Valid UTF-8 is sent as it is; every other byte,
/// and each byte of a character that would read as one of these, becomes
/// the character [`RAW_BYTE_BASE`] + byte. [`path_from_wire`] undoes it, so
/// a name the listing sent names the same file when it comes back, and
/// names that look alike stay apart. `/` and every ASCII byte are kept, so
/// the UI joins paths and names as plain strings. Every path and name in
/// Nova Files' commands and events (`show_folder`, `files_*`) is wire text.
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

/// The longest each of an `app_icon` request's names may be, in
/// characters.
pub const APP_ICON_MAX_CHARS: usize = 512;

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub matched: bool,
}

/// What the notifications list holds (feature `notifications`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationList {
    /// The newest first, at most [`MAX_NOTIFICATIONS`].
    pub items: Vec<Notification>,
    /// No pop-ups, except critical ones.
    pub do_not_disturb: bool,
    /// Apps, by [`Notification::app`], whose notifications are hidden: not
    /// in `items`, never popping up, never unread. Those from before the
    /// app was muted come back once it's unmuted; those it sends meanwhile
    /// aren't kept.
    pub muted: Vec<String>,
    /// Every app that sent one lately or is muted, the latest first, for
    /// the settings page's per-app switches.
    pub apps: Vec<NotificationApp>,
    /// Transient notifications still open: they pop up but aren't listed,
    /// and their pop-ups go once they leave this.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transient: Vec<Notification>,
}

/// How many notifications are kept; the oldest go first.
pub const MAX_NOTIFICATIONS: usize = 100;

/// An app on the notification settings page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationApp {
    /// As [`Notification::app`].
    pub name: String,
    /// As [`Notification::icon`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// As [`Notification::app_id`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
}

/// One notification an app sent (org.freedesktop.Notifications).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notification {
    /// novad's id for it, the one the app knows it by.
    pub id: u32,
    /// The app's name as it gave it, or its desktop entry's id.
    pub app: String,
    /// The app's icon: an absolute path to an image, the file it gave or
    /// the icon theme's for the name it gave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// The app's desktop file id (`discord.desktop`), when it said which.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    /// One line, plain text.
    pub summary: String,
    /// Plain text: any markup is taken out.
    pub body: String,
    /// A picture that goes with it (a sender's photo), as an absolute path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Its buttons, in the app's order. Clicking the notification itself
    /// is `has_default`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<NotificationAction>,
    /// The app does something when it's clicked (its default action).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub has_default: bool,
    pub urgency: Urgency,
    /// How far along something it reports is, 0 to 100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<u8>,
    /// How long it may show as a pop-up, as the app asked: `None` for the
    /// UI's default, 0 for until it's closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u32>,
    /// When it came, or was last replaced, in seconds since the Unix epoch.
    pub time: i64,
    /// Not seen in the list yet (`read_notifications`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unread: bool,
    /// The app that sent it is still there to hear about clicks: false for
    /// those kept from before novad started.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub live: bool,
    /// The device a low battery alert of Nova's own is about, for its
    /// icon: never set on an app's notification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<NotificationDevice>,
}

/// The device a low battery alert is about: what it is and its charge
/// when the alert was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationDevice {
    pub kind: PeripheralKind,
    pub battery: Battery,
}

/// One of a notification's buttons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationAction {
    /// What `notification_action` sends back.
    pub key: String,
    pub label: String,
}

/// How much a notification matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    /// Pops up even during do not disturb, and stays until it's closed.
    Critical,
}

/// A volume key `nova volume` stands for. The UI steps by its own
/// `Tokens.volumeStep` and stops at 100%.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeKey {
    /// One step louder.
    Up,
    /// One step quieter.
    Down,
    /// Mute, or unmute when muted.
    Mute,
}

/// What `nova screenshot` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    /// Freezes every screen and lets the user drag out a region.
    Region,
    /// The whole focused screen, right away.
    Screen,
}

/// One frozen screen, as `grim` saved it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureFrame {
    /// The output's connector name (`DP-1`), as Quickshell's `screen.name`.
    pub output: String,
    /// A PNG in novad's private runtime folder, gone once the picker closes.
    pub path: String,
}

/// A rectangle in an image's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedScreenshot {
    pub id: u64,
    /// Where it was saved.
    pub path: String,
    pub width: u32,
    pub height: u32,
    /// It's on the clipboard too (wl-copy worked).
    pub copied: bool,
}

/// What the screenshot or recording card does with a saved file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotAction {
    /// Opens it in the default app (image viewer, video player).
    Open,
    /// Opens the folder it's in.
    Folder,
    /// Deletes the file.
    Delete,
}

/// A rectangle in the compositor's layout, in logical pixels: where
/// outputs sit side by side (`screen.x` and `screen.y` in Quickshell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// The sound a recording takes, mixed into one track.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordAudio {
    /// What the default output plays.
    #[serde(default)]
    pub desktop: bool,
    /// The default input.
    #[serde(default)]
    pub mic: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveRecording {
    /// The output it's on.
    pub output: String,
    /// The part of the layout recorded, or `null` for the whole output.
    pub region: Option<LayoutRect>,
    pub audio: RecordAudio,
    /// How much has been recorded when this was sent, pauses left out.
    pub elapsed_ms: u64,
    pub paused: bool,
    /// The recorder can pause (gpu-screen-recorder can, wf-recorder can't).
    pub can_pause: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedRecording {
    pub id: u64,
    /// Where it was saved.
    pub path: String,
    /// How long it plays.
    pub duration_ms: u64,
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
    /// Locks the session (`nova lock`, feature `lock`) and answers once the
    /// compositor has covered the screens. Fails when the feature isn't
    /// ready, no UI is connected to show the lock screen, or the screens
    /// aren't covered in time.
    Lock,
    /// The compositor covered the screens with the UI's lock screen
    /// (feature `lock`).
    ReportLocked,
    /// The UI unlocked the session after the user's password checked out
    /// (feature `lock`). Unlocking is the UI's alone: this only tells novad.
    ReportUnlocked,
    /// Replies with a [`PowerMenu`]: what the power menu can do now, a
    /// [`PowerOption`] per [`PowerAction`], and when the system booted
    /// (feature `power`). Asked each time the menu opens.
    PowerOptions,
    /// Runs a power menu action (feature `power`). The menu asks the user
    /// to confirm the ones that end the session first.
    Power {
        action: PowerAction,
    },
    /// Changes the default output's volume and shows the capsule, even at
    /// either end or without an output (`nova volume`). Fails when no UI is
    /// connected to do it.
    Volume {
        key: VolumeKey,
    },
    /// Takes a screenshot (`nova screenshot`, feature `screenshot`). `region`
    /// freezes the screens and answers once the picker can show; asked again
    /// while it shows, it closes it. `screen` saves the focused screen.
    Screenshot {
        mode: CaptureMode,
    },
    /// Saves `rect` of `output`'s frozen frame, in that frame's pixels, and
    /// closes the picker (the UI).
    SaveCapture {
        output: String,
        rect: PixelRect,
    },
    /// Closes the region picker without a screenshot (the UI).
    CancelCapture,
    /// Opens, shows the folder of or deletes a screenshot saved since novad
    /// started (the screenshot card).
    ScreenshotAction {
        id: u64,
        action: ScreenshotAction,
    },
    /// `nova record` (feature `recording`): stops the recording, or when
    /// nothing is being recorded asks the UI to open the region picker for
    /// one (`pick_recording`). Fails when no UI is connected for that.
    Record,
    /// Starts recording `output`, or `region` of the layout on it (the UI,
    /// after its countdown). Answers once the recorder has started.
    StartRecording {
        output: String,
        region: Option<LayoutRect>,
        #[serde(default)]
        audio: RecordAudio,
    },
    /// Stops the recording and saves it (the bar's recording pill).
    StopRecording,
    /// Pauses or resumes the recording, when the recorder can.
    PauseRecording {
        paused: bool,
    },
    /// Opens, shows the folder of or deletes a recording saved since novad
    /// started (the recording card).
    RecordingAction {
        id: u64,
        action: ScreenshotAction,
    },
    /// Opens novad's log (feature `recording`: the recording card's Open log,
    /// after a recording didn't start).
    OpenLog,
    /// Starts or stops this connection watching the machine's resources
    /// (feature `resources`): while any connection watches, novad
    /// samples them every few seconds and sends `resources` events. A
    /// connection that closes stops watching.
    WatchResources {
        watching: bool,
    },
    /// Asks every process of an app in the latest `resources` event to
    /// close (SIGTERM), by its [`AppUsage::key`] (feature
    /// `resources`). Only the user's own processes, and never a
    /// [`AppUsage::protected`] app.
    EndApp {
        key: String,
    },
    /// Replies with what the launcher's providers find for `query`, as
    /// [`LauncherItem`]s (feature `launcher`): the providers in their slot
    /// order, each one's best first. A query that starts with a provider's
    /// trigger goes to the providers with that trigger only, without it. A
    /// blank query finds nothing.
    LauncherSearch {
        query: String,
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
    /// `new_window` asks for another one.
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
    PinApp {
        id: String,
        pinned: bool,
    },
    /// Hides a tray app from the bar, or shows it again (feature `tray`):
    /// `id` is the item's app id, or its own id when it has none (see
    /// [`Tray::hidden`]). Remembered across restarts.
    HideTrayApp {
        id: String,
        hidden: bool,
    },
    /// The tray item at `address` (as [`TrayItem::address`]) was clicked
    /// (feature `tray`): it's asked to show its window, which then takes
    /// focus.
    ActivateTrayItem {
        address: String,
    },
    /// An entry of the menu of the tray item at `address` was picked
    /// (feature `tray`): a window its app shows next takes focus, as it
    /// gets none from a menu either.
    TrayMenuPicked {
        address: String,
    },
    /// The notification `id` was clicked on `output` (feature
    /// `notifications`): the app hears of it and its window takes focus, or,
    /// when it can't hear of it, the app opens (as with `launch_app`). It
    /// stays on the list, read.
    OpenNotification {
        id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    /// The button `action` of notification `id` was clicked (feature
    /// `notifications`): the app hears of it, nothing takes focus, and it
    /// stays on the list, read.
    NotificationAction {
        id: u32,
        action: String,
    },
    /// Closes the notification `id` and takes it off the list (feature
    /// `notifications`).
    DismissNotification {
        id: u32,
    },
    /// Takes every notification off the list, or only `app`'s (feature
    /// `notifications`).
    ClearNotifications {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app: Option<String>,
    },
    /// The list was seen: no notification is unread any more (feature
    /// `notifications`).
    ReadNotifications,
    /// Turns do not disturb on or off (feature `notifications`).
    /// Remembered across restarts.
    SetDoNotDisturb {
        on: bool,
    },
    /// Mutes or unmutes `app`'s notifications (feature `notifications`).
    /// Remembered across restarts.
    MuteNotificationApp {
        app: String,
        muted: bool,
    },
    /// Shows `path` as the wallpaper on every output, or only on `output`
    /// (`nova wallpaper`): an absolute path to an image (feature
    /// `wallpaper`), or to a video or a Wallpaper Engine project folder, or
    /// a Steam Workshop ID (feature `wallpaper-engine`). Remembered across
    /// restarts until config.toml's `[wallpaper]` changes. Fails when it
    /// can't be shown.
    SetWallpaper {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    /// Replies with the [`Wallpapers`] shown now (feature `wallpaper` or
    /// `wallpaper-engine`).
    Wallpapers,
    /// Replies with the [`WallpaperFolder`]: the images in the wallpaper
    /// folder, for the carousel (feature `wallpaper`).
    WallpaperFolder,
    /// Plays or pauses the media player `player` (a [`MediaPlayer::id`]),
    /// or skips a track (feature `media`).
    MediaControl {
        player: String,
        action: MediaAction,
    },
    /// Moves the player to `position_us` into its track, kept within the
    /// track (feature `media`). Only for a player that `can_seek`.
    MediaSeek {
        player: String,
        position_us: i64,
    },
    /// Shows `player` in the bar instead of the one novad picked, until
    /// another player starts playing (feature `media`).
    MediaShow {
        player: String,
    },
    /// Brings the player's window forward where it is (feature `media`).
    /// Answers whether it found one to bring.
    MediaFocus {
        player: String,
    },
    /// The name of the workspace the player's window is on, or `null`
    /// (feature `media`), for the panel's "Go to" row.
    MediaWindow {
        player: String,
    },
    /// Turns Wi-Fi on or off (feature `network`).
    SetWifi {
        enabled: bool,
    },
    /// Asks the Wi-Fi card to look for networks now (feature `network`),
    /// when the network page opens. `scanning` says when it's done.
    ScanWifi,
    /// Connects the Wi-Fi card to `ssid` (feature `network`): with its
    /// saved profile, else a new one, with `password` for one that needs
    /// it. Answers once NetworkManager has started; how it ends comes as
    /// the [`Wifi::attempt`]. A password for a saved network replaces the
    /// saved one. A new network it never got onto isn't kept.
    ConnectWifi {
        ssid: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password: Option<WifiPassword>,
    },
    /// Disconnects the Wi-Fi card from the network it's on (feature
    /// `network`). That network doesn't connect by itself again until it's
    /// picked or the machine restarts; others still do.
    DisconnectWifi,
    /// Forgets the saved profiles of the Wi-Fi network `ssid` (feature
    /// `network`), which disconnects from it first if it's on it.
    ForgetWifi {
        ssid: String,
    },
    /// Which link carries traffic while both a cable and Wi-Fi are
    /// connected (feature `network`); the other stays connected as a
    /// backup. Remembered across restarts. NetworkManager's saved profiles
    /// aren't changed: novad lowers the link's route metric while it's
    /// connected.
    PreferNetwork {
        kind: LinkKind,
    },
    /// Starts or stops this connection watching network traffic (feature
    /// `network`): while any does, novad sends `network_traffic` every
    /// second. A connection that closes stops watching.
    WatchNetworkTraffic {
        watching: bool,
    },
    /// Turns Bluetooth on or off (feature `bluetooth`). On also lifts a
    /// software block (rfkill); a hardware switch is refused.
    BluetoothPower {
        on: bool,
    },
    /// Starts or stops this connection looking for new devices, while the
    /// Bluetooth page shows (feature `bluetooth`). Scanning runs while any
    /// connection watches, for at most two minutes from the last start.
    WatchBluetooth {
        watching: bool,
    },
    /// Connects the device at `address` (feature `bluetooth`). A new one is
    /// paired and trusted first, so it reconnects by itself later; pairing
    /// may ask the user (`Bluetooth::pairing`). Answers once it's connected,
    /// or with why it failed.
    BluetoothConnect {
        address: String,
    },
    BluetoothDisconnect {
        address: String,
    },
    /// Removes the device's pairing.
    BluetoothForget {
        address: String,
    },
    /// Answers the pairing request `id`.
    BluetoothAnswer {
        id: u64,
        answer: PairingAnswer,
    },
    /// Opens a Nova Files window on `folder`, an absolute path, with
    /// `select` (absolute paths of items in it) selected (`nova files`,
    /// feature `files`). Starts the Nova Files process when it isn't
    /// running.
    ShowFolder {
        folder: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        select: Vec<String>,
    },
    /// Sent by the Nova Files process (feature `files`): `true` once it can
    /// open windows, which novad then sends it ([`Event::FilesWindow`]),
    /// waiting ones first; `false` as it quits after its last window
    /// closes, so the next window starts a new one. A connection that
    /// closes is detached.
    AttachFiles {
        attached: bool,
    },
    /// Opens the file at `path`, an absolute path, in the app `app` (its
    /// desktop file id) or, without one, in its type's default app
    /// (feature `files`: a double-click, Enter, or a pick from "Open with").
    FilesLaunch {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app: Option<String>,
    },
    /// The apps that can open the file at `path` (feature `files`),
    /// answered with an [`OpenWith`].
    FilesOpenWith {
        path: String,
    },
    /// Makes the app `app` the default for files of the type of the one at
    /// `path` (feature `files`).
    FilesSetDefault {
        path: String,
        app: String,
    },
    /// Shows the folder at the absolute `path` in this connection's view
    /// `view` (feature `files`), in `sort` order, with hidden entries (a
    /// name starting with a dot) only when `hidden`. Whatever the view
    /// showed is replaced. Answered once the view is open, without
    /// touching the disk, so a stalled mount holds up only this view; its
    /// entries follow as [`Event::FilesPage`]s and its changes as
    /// [`Event::FilesChanged`] until `files_close`, or an
    /// [`Event::FilesClosed`] when the folder can't be read or goes.
    FilesOpen {
        view: u32,
        path: String,
        sort: FileSort,
        hidden: bool,
    },
    /// Closes this connection's view `view`. A view closes by itself when
    /// its connection does.
    FilesClose {
        view: u32,
    },
    /// The sidebar's folder tree for a window on `folder` (feature
    /// `files`), answered with a [`FolderTree`], or an error when `folder`
    /// isn't a folder. `folder` is absolute or starts at `~`. The tree
    /// starts at Home, or at `/` for a folder outside it, and is open down
    /// to `folder` except at the folders in `closed`; the folders in
    /// `opened` are open too. Hidden folders show only on the way to
    /// `folder`, or everywhere when `hidden`.
    FilesTree {
        folder: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        opened: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        closed: Vec<String>,
        hidden: bool,
    },
    /// The places on top of Nova Files' sidebar (feature `files`),
    /// answered with the [`FilePlace`]s that exist, in order.
    FilesPlaces,
    /// Copies or moves `sources` (absolute paths, wire text) into the folder `folder`
    /// as this connection's job `job` (feature `files`). Answered once the
    /// job is started, without touching the disk; how it goes follows as
    /// [`Event::FilesTransfer`]s. An item copied into the folder it's
    /// already in gets a number ("cover (2).png"); one moved there stays.
    /// A name already taken asks first, with a [`Clash`]. A job ends by
    /// itself when its connection closes.
    FilesTransfer {
        job: u32,
        mode: TransferMode,
        sources: Vec<String>,
        folder: String,
    },
    /// What dropping `sources` (absolute paths, wire text) on the folder
    /// `folder` does (feature `files`), answered with a [`TransferMode`],
    /// or null when nothing can be dropped there, as Explorer decides: on
    /// the folder's drive it moves, onto another it copies, and `copy`
    /// (Ctrl) copies anyway. A folder never goes into itself or one of its
    /// own folders, and a move into the folder the items are in does
    /// nothing. Nothing is touched: `files_transfer` does the drop.
    FilesDrop {
        sources: Vec<String>,
        folder: String,
        copy: bool,
    },
    /// Stops this connection's job `job`. What it already copied or moved
    /// stays; a file it was halfway through is removed.
    FilesTransferCancel {
        job: u32,
    },
    /// Answers the name clash `clash` that job `job` asks about. With
    /// `all`, the same answer goes to the job's later clashes of the same
    /// kind (file or folder) without asking.
    FilesTransferAnswer {
        job: u32,
        clash: u32,
        answer: ClashAnswer,
        all: bool,
    },
    /// Moves the items at `paths` (absolute) to the trash (feature
    /// `files`), each to its own drive's, as the freedesktop Trash spec
    /// says, so other apps see them. All are tried; the error names those
    /// that weren't moved. One trashing is one step to undo; with
    /// `continues`, these paths join the step the previous `files_trash`
    /// started (a selection too big for one request).
    FilesTrash {
        paths: Vec<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        continues: bool,
    },
    /// Undoes the last copy, move or trashing done in Nova Files this
    /// session (feature `files`, Ctrl+Z), answered with whether it started
    /// a move as this connection's job `job`, which then reports like a
    /// `files_transfer`. Anything else is undone before the answer; with
    /// nothing to undo, nothing happens. See [`Event::FilesHistory`].
    FilesUndo {
        job: u32,
    },
    /// Does the last undone step again (feature `files`, Ctrl+Y), answered
    /// like [`Command::FilesUndo`].
    FilesRedo {
        job: u32,
    },
    /// Deletes the items at `paths` (absolute) for good, folders with all
    /// they hold (feature `files`, Shift+Delete).
    FilesDelete {
        paths: Vec<String>,
    },
    /// What the trash holds (feature `files`), answered with
    /// [`TrashItem`]s, the last trashed first.
    FilesTrashList,
    /// Puts the trashed items `ids` back where they came from (feature
    /// `files`). One whose place is taken stays in the trash.
    FilesRestore {
        ids: Vec<String>,
    },
    /// Deletes the trashed items `ids` for good (feature `files`).
    FilesTrashDelete {
        ids: Vec<String>,
    },
    /// Deletes everything in the trash for good (feature `files`).
    FilesEmptyTrash,
    /// Mounts the drive `drive` (a [`Drive::id`], feature `files`), if it
    /// isn't, and answers with the folder it's mounted at.
    FilesMount {
        drive: String,
    },
    /// Ejects the drive `drive` (a [`Drive::id`] of a removable one,
    /// feature `files`): unmounts every filesystem on its device, writing
    /// what's left first, then powers it off. Answered with an
    /// [`EjectOutcome`]; once ejected, a "Safe To Remove Hardware"
    /// notification says so.
    FilesEject {
        drive: String,
    },
    /// Finds files by name (feature `files`): every name under Home and
    /// the drives holding each word of `query`, case aside, hidden folders
    /// included. Answered with a [`FileSearch`] from novad's index, without
    /// reading folders.
    FilesSearch {
        query: String,
    },
    /// What the Properties window shows about the item at `path` (an
    /// absolute path, feature `files`), answered with [`FileProperties`].
    /// A link is described by what it points to, as in its folder's
    /// listing, and says where that is.
    FilesProperties {
        path: String,
    },
    /// Counts what `paths` (absolute) hold, folders all the way down, as
    /// this connection's job `job` (feature `files`): the Properties
    /// window's size. Answered once the job is started, without touching
    /// the disk; the counts follow as [`Event::FilesMeasured`]s. Links
    /// aren't followed, other drives mounted inside an item aren't
    /// counted, and folders that can't be read are skipped. A job
    /// ends by itself when its connection closes.
    FilesMeasure {
        job: u32,
        paths: Vec<String>,
    },
    /// Stops this connection's measuring job `job`; it sends nothing more.
    FilesMeasureCancel {
        job: u32,
    },
    /// Sets who may read, change and run the item at `path` (absolute,
    /// feature `files`; a link's target), keeping its other mode bits.
    /// Only its owner can.
    FilesSetAccess {
        path: String,
        access: Access,
    },
}

/// What the Properties window shows about one item
/// ([`Command::FilesProperties`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileProperties {
    /// The folder it's in, absolute, as wire text ([`wire_text`]).
    pub folder: String,
    /// The item itself, as its folder's listing has it, but its type read
    /// from its first bytes when its name doesn't say.
    pub entry: FileEntry,
    /// Where a symbolic link points, as written in it (wire text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_target: Option<String>,
    /// What a file takes on its drive, in bytes; none for a folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_bytes: Option<u64>,
    /// When it was made, when the file system says, and last read, in
    /// milliseconds since 1970.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accessed_ms: Option<i64>,
    /// Its owner's and group's names, or their numbers when the user
    /// database has none.
    pub owner: String,
    pub group: String,
    pub access: Access,
    /// The user novad runs as owns it, so `files_set_access` may change it.
    pub yours: bool,
    /// A drive is mounted here: how big it is and how much is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive: Option<DriveSpace>,
}

/// Who may do what with an item: its owner, its group and everyone else.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Access {
    pub owner: Rights,
    pub group: Rights,
    pub others: Rights,
}

/// Read, change, and run (for a folder: go into) an item.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rights {
    pub read: bool,
    pub write: bool,
    pub run: bool,
}

/// How big a mounted drive is and how much of it is used, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriveSpace {
    pub size: u64,
    pub used: u64,
}

/// How far a `files_measure` job got ([`Event::FilesMeasured`]): what's in
/// its items, the items themselves not counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measured {
    pub job: u32,
    /// Files (and links) and folders found inside the items so far.
    pub files: u64,
    pub folders: u64,
    /// The bytes of every file, the items' own included, and what they
    /// take on their drives.
    pub bytes: u64,
    pub disk_bytes: u64,
    /// The last event of the job: the counts are complete.
    pub done: bool,
}

/// A drive in Nova Files' sidebar ([`Event::FilesDrives`]): one
/// filesystem, on a disk inside the machine or a USB stick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drive {
    /// What `files_mount` and `files_eject` name it by.
    pub id: String,
    pub name: String,
    /// It can be unplugged: a USB stick or card, which can be ejected.
    pub removable: bool,
    /// The folder it's mounted at, as wire text ([`wire_text`]); none
    /// while it isn't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount: Option<String>,
    /// Its size, in bytes.
    pub size: u64,
    /// How much of it is used, in bytes, while it's mounted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used: Option<u64>,
}

/// How `files_eject` went, when it didn't fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EjectOutcome {
    /// Safe to unplug.
    Ejected,
    /// Something still has files open on it: it stays mounted.
    Busy,
}

/// The answer to [`Command::FilesTree`]: the folder as an absolute path,
/// and the tree's rows, top to bottom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FolderTree {
    pub folder: String,
    pub rows: Vec<TreeRow>,
}

/// One folder in the sidebar's tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeRow {
    /// Its name; `/` for the root.
    pub name: String,
    /// Absolute.
    pub path: String,
    /// How far in from the tree's top: 0 for Home or `/`.
    pub depth: u32,
    /// Its subfolders are the rows that follow.
    pub open: bool,
    /// It has no subfolders to show.
    pub leaf: bool,
    /// It's a dot folder or inside one.
    pub hidden: bool,
}

/// One of the places on top of Nova Files' sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilePlace {
    pub kind: PlaceKind,
    /// Absolute.
    pub path: String,
}

/// Which place a [`FilePlace`] is: Home, one of the user's folders from
/// `user-dirs.dirs`, or the trash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind {
    Home,
    Desktop,
    Documents,
    Downloads,
    Pictures,
    /// The trash, at [`FILES_TRASH_PLACE`]: not a folder, listed with
    /// `files_trash_list`.
    Trash,
}

/// The trash place's path ([`PlaceKind::Trash`]): a window there shows the
/// trash instead of a folder.
pub const FILES_TRASH_PLACE: &str = "trash:///";

/// An item in the trash (feature `files`, [`Command::FilesTrashList`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrashItem {
    /// Names it to `files_restore` and `files_trash_delete`.
    pub id: String,
    /// The folder it was trashed from, which it goes back to, as wire text
    /// ([`wire_text`]).
    pub folder: String,
    /// When it was trashed, in milliseconds since 1970, when the trash
    /// says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_ms: Option<i64>,
    /// The item, under the name it had.
    pub entry: FileEntry,
}

/// The answer to [`Command::FilesSearch`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSearch {
    /// The index is built; until then nothing is found.
    pub ready: bool,
    /// How many names fit, of which `results` are the best, at most
    /// [`FILES_SEARCH_MAX_RESULTS`].
    pub total: u32,
    /// Best first: whole names, then names starting with the first word,
    /// then a word in the name starting with it, then the rest; within
    /// each, names that aren't hidden (themselves or by a folder they're
    /// in) first, then shorter names.
    pub results: Vec<FoundFile>,
}

/// A file a search found ([`FileSearch`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoundFile {
    /// The folder it's in, an absolute path, as wire text ([`wire_text`]).
    pub folder: String,
    pub entry: FileEntry,
    /// A folder it's in is hidden (its own name is `entry.hidden`).
    pub in_hidden_folder: bool,
    /// Where the query's words are in its name, in order.
    pub marks: Vec<TextMark>,
}

/// A stretch of text to highlight, in UTF-16 code units, what QML's
/// strings count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextMark {
    pub start: u32,
    pub length: u32,
}

/// The order of a folder view (`files_open`). Folders always come before
/// files; within each, entries are ordered by `key`, then by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSort {
    pub key: FileSortKey,
    pub descending: bool,
}

/// What a folder view is ordered by. Names compare the way people read
/// them: case aside, and numbers by their value ("file 2" before "file 10").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSortKey {
    Name,
    /// The MIME type.
    Kind,
    Size,
    Modified,
}

/// One page of a folder view ([`Event::FilesPage`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FolderPage {
    pub view: u32,
    /// The folder, as `files_open` named it.
    pub path: String,
    pub start: u32,
    /// How many entries the view holds once every page is in.
    pub total: u32,
    pub entries: Vec<FileEntry>,
    /// The archive the folder is in, when it's inside one (wire text):
    /// it's read-only, and extracting it is what can be done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<String>,
}

/// An entry of a folder view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    /// As wire text ([`path_to_wire`]): joined to the view's path, it names
    /// the entry in every files request.
    pub name: String,
    /// A folder, or a link to one.
    pub dir: bool,
    /// A symbolic link; `dir`, `mime`, `size` and `modified_ms` describe
    /// what it points to, when that exists.
    pub link: bool,
    /// Its name starts with a dot.
    pub hidden: bool,
    /// An archive Nova Files opens like a folder (zip, tar): `files_open`
    /// shows what's in it, read-only.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archive: bool,
    /// Its type in the shared MIME database, from its name (`inode/directory`
    /// for a folder).
    pub mime: String,
    /// In bytes; none for a folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Last modified, in milliseconds since 1970.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_ms: Option<i64>,
}

/// One change to a folder view ([`Event::FilesChanged`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum FolderChange {
    /// `entry` is new, at `index`.
    Insert { index: u32, entry: FileEntry },
    /// The entry at `index` is gone.
    Remove { index: u32 },
    /// The entry at `index` changed and keeps its place.
    Update { index: u32, entry: FileEntry },
}

/// A kind of step Nova Files can undo and redo ([`Event::FilesHistory`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStep {
    /// Copies made; undone, they go to the trash.
    Copy,
    /// Items moved; undone, they move back.
    Move,
    /// Items moved to the trash; undone, they're restored.
    Trash,
}

/// What a `files_transfer` job does with its items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferMode {
    Copy,
    Move,
    /// Extracts each archive among `sources` into a new folder named after
    /// it ("photos.zip" into "photos") in `folder`, merging with one already
    /// there ("Extract all").
    Extract,
    /// Extracts each archive's contents straight into `folder`.
    ExtractHere,
    /// Packs `sources` into one new zip in `folder`, named after the first
    /// ("Compress to ZIP"); a taken name gets a number.
    Compress,
}

/// How a copy or move job is going ([`Event::FilesTransfer`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transfer {
    pub job: u32,
    pub mode: TransferMode,
    pub state: TransferState,
    /// How many items it was given.
    pub items: u32,
    /// The first of them, by name (wire text).
    pub name: String,
    /// Bytes copied or moved so far, of `total_bytes`; both 0 while
    /// counting.
    pub done_bytes: u64,
    pub total_bytes: u64,
    /// The name clash it waits on, while `asking`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clash: Option<Clash>,
    /// Why it stopped, once `failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferState {
    /// Measuring what there is to copy.
    Counting,
    Running,
    /// Waiting for `files_transfer_answer`.
    Asking,
    /// The last event of a job: it finished, was cancelled or failed.
    Done,
    Cancelled,
    Failed,
}

/// A name a copy or move would take that's already taken in its folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clash {
    /// The job's number for it, for `files_transfer_answer`.
    pub id: u32,
    /// The taken name (wire text).
    pub name: String,
    /// A folder landing on a folder, which can be merged; otherwise the
    /// one already there can be replaced.
    pub folder: bool,
    /// What's there now.
    pub original: ClashItem,
    /// What would take its place.
    pub replacement: ClashItem,
    /// How many more clashes of the same kind the job knows of after this
    /// one: what `all` would answer too.
    pub others: u32,
}

/// One side of a [`Clash`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClashItem {
    /// A file's size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// How many entries a folder holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<u32>,
    /// Last modified, in milliseconds since 1970.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_ms: Option<i64>,
}

/// The answer to a [`Clash`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClashAnswer {
    /// The one there now goes; only for a file clash.
    Replace,
    /// The folders' contents come together, asking about clashes inside;
    /// only for a folder clash.
    Merge,
    /// The item isn't copied or moved.
    Skip,
}

/// Whether a `set_wallpaper` path is a Steam Workshop ID: only digits.
pub fn is_workshop_id(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// What `wallpapers` returns: the wallpaper every output shows, and the
/// outputs that show one of their own. Absolute paths or Workshop IDs;
/// empty when none is set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wallpapers {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub outputs: std::collections::BTreeMap<String, String>,
}

/// What `wallpaper_folder` returns: the wallpaper folder (config.toml's
/// `[wallpaper] folder`) and the images in it, sorted by name, at most
/// [`WALLPAPER_FOLDER_MAX_IMAGES`]. Absolute paths; no images when the folder
/// doesn't exist.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WallpaperFolder {
    pub folder: String,
    pub images: Vec<String>,
}

/// The most images `wallpaper_folder` lists: a carousel with more is no use,
/// and a reply's size is capped.
pub const WALLPAPER_FOLDER_MAX_IMAGES: usize = 1000;

/// What the power menu does, in the order it lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerAction {
    Lock,
    LogOut,
    Suspend,
    Reboot,
    PowerOff,
}

impl PowerAction {
    pub const ALL: [PowerAction; 5] =
        [PowerAction::Lock, PowerAction::LogOut, PowerAction::Suspend, PowerAction::Reboot, PowerAction::PowerOff];
}

/// Whether a power action can run now, and why not when it can't.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerOption {
    pub action: PowerAction,
    pub available: bool,
    /// For people, shown under the dimmed action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The machine and its user, as the quick settings header shows them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Host {
    /// The machine's host name.
    pub host: String,
    /// What to call the user: config.toml's `[quick-settings] name` ("" for
    /// none), else their login name.
    pub user: String,
    /// When the machine booted, in seconds since the Unix epoch.
    pub booted_at: i64,
    pub os: OperatingSystem,
}

/// The distribution, from os-release.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatingSystem {
    /// `ID`, like `nixos` or `arch`; `linux` when unset.
    pub id: String,
    /// `ID_LIKE`: the distributions it derives from, closest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub id_like: Vec<String>,
    /// `NAME`, like `NixOS`.
    pub name: String,
    /// `LOGO`: the icon theme's file for the logo it names, when set and
    /// the theme has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
}

/// One look at what the machine is busy with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    /// Share of all CPU time in use since the last look, 0 to 100.
    pub cpu: f32,
    /// Memory in use (not available to start programs), in bytes.
    pub memory_used: u64,
    pub memory_total: u64,
    /// Share of the GPU in use, 0 to 100, when the driver reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<f32>,
    /// The CPU's temperature in °C, when a sensor reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// CPU and memory over the last few minutes, oldest first, one sample
    /// every [`RESOURCE_HISTORY_INTERVAL_SECS`].
    pub history: Vec<ResourceSample>,
    /// How many samples `history` holds once it's full.
    pub history_size: usize,
    /// The apps using the most memory, most first. An app is every process
    /// running the same program.
    pub apps: Vec<AppUsage>,
}

/// Seconds between two of [`Resources::history`]'s samples.
pub const RESOURCE_HISTORY_INTERVAL_SECS: u64 = 5;

/// CPU and memory at one point in time, each 0 to 100.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceSample {
    pub cpu: f32,
    pub memory: f32,
}

/// One app's processes together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppUsage {
    /// The program's name, from its file (`.firefox-wrapped` is `firefox`);
    /// `Nova` for novad and its UI.
    pub name: String,
    /// What `end_app` takes: the program's path, `comm:<name>` for a
    /// program novad can't see, or `nova` for Nova.
    pub key: String,
    /// How many processes run it.
    pub processes: u32,
    /// Their share of all CPU time since the last look, 0 to 100.
    pub cpu: f32,
    /// Their resident memory together, in bytes.
    pub memory: u64,
    /// Ending it isn't offered: Nova itself, the compositor, or another
    /// user's.
    pub protected: bool,
}

/// What `power_options` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerMenu {
    pub options: Vec<PowerOption>,
    /// When the system booted, in Unix seconds, for the menu's uptime; left
    /// out when novad can't tell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub booted_at: Option<u64>,
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
    /// No feature with a UI is on, so novad doesn't run Quickshell.
    NotNeeded,
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
    /// accept with `cargo insta review`, then run `just protocol-js`.
    #[test]
    fn wire_format() {
        let lines = [
            encode(Event::Hello { daemon: "0.1.0".into() }),
            encode(Event::Features {
                features: vec![
                    FeatureState {
                        id: "bar".into(),
                        status: FeatureStatus::Ready,
                        missing: vec![],
                        slots: vec![Slot { host: "bar.center".into(), view: "central-pill".into(), order: 10 }],
                    },
                    FeatureState {
                        id: "recording".into(),
                        status: FeatureStatus::MissingDeps,
                        missing: vec!["gpu-screen-recorder|wf-recorder".into()],
                        slots: vec![],
                    },
                    FeatureState { id: "weather".into(), status: FeatureStatus::Off, missing: vec![], slots: vec![] },
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
                        Topic::Lock,
                        Topic::Volume,
                        Topic::Screenshot,
                        Topic::Recording,
                        Topic::Host,
                        Topic::Resources,
                        Topic::Launcher,
                        Topic::Tray,
                        Topic::Notifications,
                        Topic::Media,
                        Topic::Peripherals,
                        Topic::Network,
                        Topic::Bluetooth,
                        Topic::Files,
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
            encode(Request { id: Some(16), command: Command::Lock }),
            encode(Event::Lock { locked: true }),
            encode(Request { id: Some(20), command: Command::ReportLocked }),
            encode(Request { id: Some(17), command: Command::ReportUnlocked }),
            encode(Event::Lock { locked: false }),
            encode(Request { id: Some(18), command: Command::PowerOptions }),
            encode(Response {
                id: 18,
                outcome: Outcome::Result(
                    serde_json::to_value(PowerMenu {
                        options: vec![
                            PowerOption { action: PowerAction::Lock, available: true, reason: None },
                            PowerOption {
                                action: PowerAction::Suspend,
                                available: false,
                                reason: Some("suspend is turned off".into()),
                            },
                        ],
                        booted_at: Some(1_761_350_400),
                    })
                    .unwrap(),
                ),
            }),
            encode(Request { id: Some(19), command: Command::Power { action: PowerAction::PowerOff } }),
            encode(Request { id: None, command: Command::Toggle { panel: Panel::Power } }),
            encode(Request { id: Some(21), command: Command::Volume { key: VolumeKey::Up } }),
            encode(Event::Volume { key: VolumeKey::Down }),
            encode(Event::Volume { key: VolumeKey::Mute }),
            encode(Request { id: Some(30), command: Command::Screenshot { mode: CaptureMode::Region } }),
            encode(Request { id: Some(31), command: Command::Screenshot { mode: CaptureMode::Screen } }),
            encode(Event::Capture {
                frames: vec![CaptureFrame {
                    output: "DP-1".into(),
                    path: "/run/user/1000/nova/capture/DP-1.ppm".into(),
                }],
            }),
            encode(Request {
                id: Some(32),
                command: Command::SaveCapture {
                    output: "DP-1".into(),
                    rect: PixelRect { x: 1460, y: 150, width: 960, height: 560 },
                },
            }),
            encode(Request { id: Some(33), command: Command::CancelCapture }),
            encode(Event::CaptureEnded),
            encode(Event::ScreenshotSaved(SavedScreenshot {
                id: 1,
                path: "/home/me/Pictures/Screenshots/Screenshot 2026-10-25 15-03-41.png".into(),
                width: 960,
                height: 560,
                copied: true,
            })),
            encode(Request {
                id: Some(34),
                command: Command::ScreenshotAction { id: 1, action: ScreenshotAction::Open },
            }),
            encode(Request {
                id: None,
                command: Command::ScreenshotAction { id: 1, action: ScreenshotAction::Folder },
            }),
            encode(Request {
                id: None,
                command: Command::ScreenshotAction { id: 1, action: ScreenshotAction::Delete },
            }),
            encode(Request { id: Some(35), command: Command::Record }),
            encode(Event::PickRecording),
            encode(Request {
                id: Some(36),
                command: Command::StartRecording {
                    output: "DP-1".into(),
                    region: Some(LayoutRect { x: 2560, y: -40, width: 960, height: 540 }),
                    audio: RecordAudio { desktop: true, mic: false },
                },
            }),
            encode(Event::Recording(Some(ActiveRecording {
                output: "DP-1".into(),
                region: Some(LayoutRect { x: 2560, y: -40, width: 960, height: 540 }),
                audio: RecordAudio { desktop: true, mic: false },
                elapsed_ms: 12_400,
                paused: false,
                can_pause: true,
            }))),
            encode(Request { id: Some(37), command: Command::PauseRecording { paused: true } }),
            encode(Request { id: Some(38), command: Command::StopRecording }),
            encode(Event::Recording(None)),
            encode(Event::RecordingSaved(SavedRecording {
                id: 1,
                path: "/home/me/Videos/Recordings/Recording 2026-10-25 18-03-41.mp4".into(),
                duration_ms: 42_000,
            })),
            encode(Request { id: None, command: Command::RecordingAction { id: 1, action: ScreenshotAction::Open } }),
            encode(Request { id: None, command: Command::OpenLog }),
            encode(Event::Host(host())),
            encode(Request { id: Some(22), command: Command::WatchResources { watching: true } }),
            encode(Event::Resources(resources())),
            encode(Request { id: Some(23), command: Command::EndApp { key: "/usr/lib/firefox/firefox".into() } }),
            encode(Request { id: None, command: Command::Toggle { panel: Panel::QuickSettings } }),
            encode(Event::Launcher(launcher())),
            encode(Request { id: Some(24), command: Command::LauncherSearch { query: "fi".into() } }),
            encode(Response {
                id: 24,
                outcome: Outcome::Result(
                    serde_json::to_value(vec![LauncherItem {
                        provider: PROVIDER_APPS.into(),
                        id: "org.mozilla.firefox.desktop".into(),
                        name: vec![
                            TextRun { text: "Fi".into(), matched: true },
                            TextRun { text: "refox".into(), matched: false },
                        ],
                        description: Some("Web Browser".into()),
                        icon: None,
                    }])
                    .unwrap(),
                ),
            }),
            encode(Request {
                id: Some(44),
                command: Command::AppIcon {
                    icon_name: "".into(),
                    program: "/nix/store/x-zen/lib/zen/.zen-wrapped".into(),
                    name: "Zen".into(),
                },
            }),
            encode(Response { id: 44, outcome: Outcome::Result(json!("zen-browser")) }),
            encode(Request {
                id: Some(25),
                command: Command::LauncherRun {
                    provider: PROVIDER_APPS.into(),
                    id: "foot.desktop".into(),
                    output: Some("DP-1".into()),
                    new_window: true,
                },
            }),
            encode(Request { id: Some(26), command: Command::PinApp { id: "foot.desktop".into(), pinned: true } }),
            encode(Request { id: Some(27), command: Command::HideTrayApp { id: "steam".into(), hidden: true } }),
            encode(Request {
                id: Some(28),
                command: Command::ActivateTrayItem { address: ":1.42/StatusNotifierItem".into() },
            }),
            encode(Request {
                id: Some(29),
                command: Command::TrayMenuPicked { address: ":1.42/StatusNotifierItem".into() },
            }),
            encode(Request { id: None, command: Command::Toggle { panel: Panel::Launcher } }),
            encode(Event::Tray(tray())),
            encode(Event::Notifications(notifications())),
            encode(Event::NotificationPopup(notifications().items[0].clone())),
            encode(Request { id: Some(40), command: Command::NotificationAction { id: 7, action: "open".into() } }),
            encode(Request { id: None, command: Command::OpenNotification { id: 7, output: Some("DP-1".into()) } }),
            encode(Request { id: Some(41), command: Command::DismissNotification { id: 7 } }),
            encode(Request { id: None, command: Command::ClearNotifications { app: Some("Discord".into()) } }),
            encode(Request { id: None, command: Command::ClearNotifications { app: None } }),
            encode(Request { id: None, command: Command::ReadNotifications }),
            encode(Request { id: Some(42), command: Command::SetDoNotDisturb { on: true } }),
            encode(Request {
                id: Some(43),
                command: Command::MuteNotificationApp { app: "Discord".into(), muted: true },
            }),
            encode(Request {
                id: Some(50),
                command: Command::SetWallpaper { path: "/home/luna/Pictures/dusk.jpg".into(), output: None },
            }),
            encode(Request {
                id: Some(51),
                command: Command::SetWallpaper {
                    path: "/home/luna/Pictures/tide.jpg".into(),
                    output: Some("DP-2".into()),
                },
            }),
            encode(Request { id: Some(52), command: Command::Wallpapers }),
            encode(Response {
                id: 52,
                outcome: Outcome::Result(
                    serde_json::to_value(Wallpapers {
                        path: Some("/home/luna/Pictures/dusk.jpg".into()),
                        outputs: [("DP-2".into(), "/home/luna/Pictures/tide.jpg".into())].into(),
                    })
                    .unwrap(),
                ),
            }),
            encode(Request { id: Some(53), command: Command::WallpaperFolder }),
            encode(Response {
                id: 53,
                outcome: Outcome::Result(
                    serde_json::to_value(WallpaperFolder {
                        folder: "/home/luna/Pictures/Wallpapers".into(),
                        images: vec![
                            "/home/luna/Pictures/Wallpapers/dusk.jpg".into(),
                            "/home/luna/Pictures/Wallpapers/tide.jpg".into(),
                        ],
                    })
                    .unwrap(),
                ),
            }),
            encode(Request { id: None, command: Command::Toggle { panel: Panel::Wallpaper } }),
            encode(Event::Media(media())),
            encode(Request {
                id: Some(45),
                command: Command::MediaControl {
                    player: "org.mpris.MediaPlayer2.spotify".into(),
                    action: MediaAction::PlayPause,
                },
            }),
            encode(Request {
                id: None,
                command: Command::MediaControl {
                    player: "org.mpris.MediaPlayer2.spotify".into(),
                    action: MediaAction::Next,
                },
            }),
            encode(Request {
                id: None,
                command: Command::MediaControl {
                    player: "org.mpris.MediaPlayer2.spotify".into(),
                    action: MediaAction::Previous,
                },
            }),
            encode(Request {
                id: Some(46),
                command: Command::MediaSeek {
                    player: "org.mpris.MediaPlayer2.spotify".into(),
                    position_us: 84_000_000,
                },
            }),
            encode(Request {
                id: Some(47),
                command: Command::MediaShow { player: "org.mpris.MediaPlayer2.firefox.instance_1_42".into() },
            }),
            encode(Request {
                id: Some(48),
                command: Command::MediaFocus { player: "org.mpris.MediaPlayer2.spotify".into() },
            }),
            encode(Response { id: 48, outcome: Outcome::Result(json!(true)) }),
            encode(Request {
                id: Some(49),
                command: Command::MediaWindow { player: "org.mpris.MediaPlayer2.spotify".into() },
            }),
            encode(Response { id: 49, outcome: Outcome::Result(json!("3")) }),
            encode(Event::Peripherals(peripherals())),
            encode(Event::Peripherals(Peripherals { devices: vec![] })),
            encode(Event::Network(network())),
            encode(Event::Network(Network { running: false, primary: None, prefer: None, wired: vec![], wifi: None })),
            encode(Request { id: Some(50), command: Command::SetWifi { enabled: false } }),
            encode(Request { id: Some(51), command: Command::ScanWifi }),
            encode(Request {
                id: Some(52),
                command: Command::ConnectWifi {
                    ssid: "Home".into(),
                    password: Some(WifiPassword::new("correct horse")),
                },
            }),
            encode(Request { id: Some(53), command: Command::ConnectWifi { ssid: "Cafe".into(), password: None } }),
            encode(Request { id: Some(54), command: Command::PreferNetwork { kind: LinkKind::Wired } }),
            encode(Request { id: Some(56), command: Command::DisconnectWifi }),
            encode(Request { id: Some(57), command: Command::ForgetWifi { ssid: "Cafe".into() } }),
            encode(Request { id: Some(55), command: Command::WatchNetworkTraffic { watching: true } }),
            encode(Event::NetworkTraffic(NetworkTraffic {
                down: vec![0, 1_250_000, 3_400_000],
                up: vec![0, 42_000, 61_000],
                today_down: 4_200_000_000,
                today_up: 310_000_000,
            })),
            encode(Event::Bluetooth(bluetooth())),
            encode(Request { id: Some(60), command: Command::BluetoothPower { on: true } }),
            encode(Request { id: Some(61), command: Command::WatchBluetooth { watching: true } }),
            encode(Request {
                id: Some(62),
                command: Command::BluetoothConnect { address: "9C:2A:70:11:22:33".into() },
            }),
            encode(Request {
                id: Some(63),
                command: Command::BluetoothDisconnect { address: "00:1B:66:AA:BB:CC".into() },
            }),
            encode(Request { id: Some(64), command: Command::BluetoothForget { address: "00:1B:66:AA:BB:CC".into() } }),
            encode(Request {
                id: Some(65),
                command: Command::BluetoothAnswer { id: 3, answer: PairingAnswer::Accept },
            }),
            encode(Request {
                id: Some(66),
                command: Command::BluetoothAnswer { id: 4, answer: PairingAnswer::Code("0000".into()) },
            }),
            encode(Request {
                id: Some(70),
                command: Command::FilesLaunch { path: "/home/me/photo.png".into(), app: None },
            }),
            encode(Request {
                id: Some(71),
                command: Command::FilesLaunch {
                    path: "/home/me/photo.png".into(),
                    app: Some("org.gnome.Loupe.desktop".into()),
                },
            }),
            encode(Request { id: Some(72), command: Command::FilesOpenWith { path: "/home/me/photo.png".into() } }),
            encode(Response {
                id: 72,
                outcome: Outcome::Result(
                    serde_json::to_value(OpenWith {
                        mime: "image/png".into(),
                        default: Some("org.gnome.Loupe.desktop".into()),
                        apps: vec![App {
                            id: "org.gnome.Loupe.desktop".into(),
                            name: "Image Viewer".into(),
                            description: None,
                            icon: Some("/icons/loupe.svg".into()),
                        }],
                    })
                    .unwrap(),
                ),
            }),
            encode(Request {
                id: Some(73),
                command: Command::FilesSetDefault {
                    path: "/home/me/photo.png".into(),
                    app: "org.gnome.Loupe.desktop".into(),
                },
            }),
            encode(Request {
                id: Some(74),
                command: Command::FilesOpen {
                    view: 1,
                    path: "/home/user/Pictures".into(),
                    sort: FileSort { key: FileSortKey::Modified, descending: true },
                    hidden: false,
                },
            }),
            encode(Request { id: Some(75), command: Command::FilesClose { view: 1 } }),
            encode(Request {
                id: Some(76),
                command: Command::FilesTree {
                    folder: "~/.local/share".into(),
                    opened: vec!["/home/user/Projects".into()],
                    closed: vec![],
                    hidden: false,
                },
            }),
            encode(Response {
                id: 76,
                outcome: Outcome::Result(
                    serde_json::to_value(FolderTree {
                        folder: "/home/user/.local/share".into(),
                        rows: vec![
                            TreeRow {
                                name: "user".into(),
                                path: "/home/user".into(),
                                depth: 0,
                                open: true,
                                leaf: false,
                                hidden: false,
                            },
                            TreeRow {
                                name: ".local".into(),
                                path: "/home/user/.local".into(),
                                depth: 1,
                                open: true,
                                leaf: false,
                                hidden: true,
                            },
                            TreeRow {
                                name: "share".into(),
                                path: "/home/user/.local/share".into(),
                                depth: 2,
                                open: false,
                                leaf: false,
                                hidden: true,
                            },
                        ],
                    })
                    .unwrap(),
                ),
            }),
            encode(Request { id: Some(77), command: Command::FilesPlaces }),
            encode(Response {
                id: 77,
                outcome: Outcome::Result(
                    serde_json::to_value(vec![FilePlace { kind: PlaceKind::Home, path: "/home/user".into() }]).unwrap(),
                ),
            }),
            encode(Event::FilesPage(FolderPage {
                view: 1,
                path: "/home/user/Pictures".into(),
                archive: None,
                start: 0,
                total: 2,
                entries: vec![
                    FileEntry {
                        name: "Screenshots".into(),
                        dir: true,
                        link: false,
                        hidden: false,
                        archive: false,
                        mime: "inode/directory".into(),
                        size: None,
                        modified_ms: Some(1_793_000_000_000),
                    },
                    FileEntry {
                        name: "cover.png".into(),
                        dir: false,
                        link: false,
                        hidden: false,
                        archive: false,
                        mime: "image/png".into(),
                        size: Some(48_213),
                        modified_ms: Some(1_792_900_000_000),
                    },
                ],
            })),
            encode(Event::FilesChanged {
                view: 1,
                changes: vec![
                    FolderChange::Remove { index: 1 },
                    FolderChange::Insert {
                        index: 1,
                        entry: FileEntry {
                            name: "cover (2).png".into(),
                            dir: false,
                            link: false,
                            hidden: false,
                            archive: false,
                            mime: "image/png".into(),
                            size: Some(48_213),
                            modified_ms: Some(1_793_000_100_000),
                        },
                    },
                    FolderChange::Update {
                        index: 0,
                        entry: FileEntry {
                            name: "Screenshots".into(),
                            dir: true,
                            link: true,
                            hidden: false,
                            archive: false,
                            mime: "inode/directory".into(),
                            size: None,
                            modified_ms: None,
                        },
                    },
                ],
            }),
            encode(Event::FilesClosed { view: 1, error: "/home/user/Pictures doesn't exist".into() }),
            encode(Event::FilesLook(FilesLook { frame: 0, list: 100 })),
            encode(Event::FilesDrives {
                drives: vec![
                    Drive {
                        id: "nvme0n1p2".into(),
                        name: "System".into(),
                        removable: false,
                        mount: Some("/".into()),
                        size: 1_000_000_000_000,
                        used: Some(580_000_000_000),
                    },
                    Drive {
                        id: "sdb1".into(),
                        name: "SANDISK".into(),
                        removable: true,
                        mount: None,
                        size: 32_000_000_000,
                        used: None,
                    },
                ],
            }),
            encode(Request { id: Some(79), command: Command::FilesMount { drive: "sdb1".into() } }),
            encode(Response { id: 79, outcome: Outcome::Result(json!("/run/media/user/SANDISK")) }),
            encode(Request { id: Some(80), command: Command::FilesEject { drive: "sdb1".into() } }),
            encode(Response { id: 80, outcome: Outcome::Result(serde_json::to_value(EjectOutcome::Busy).unwrap()) }),
            encode(Request {
                id: Some(76),
                command: Command::FilesTransfer {
                    job: 2,
                    mode: TransferMode::Copy,
                    sources: vec!["/home/user/Pictures/cover.png".into()],
                    folder: "/run/media/user/SANDISK".into(),
                },
            }),
            encode(Request { id: Some(77), command: Command::FilesTransferCancel { job: 2 } }),
            encode(Request {
                id: Some(78),
                command: Command::FilesTransferAnswer { job: 2, clash: 1, answer: ClashAnswer::Replace, all: true },
            }),
            encode(Event::FilesTransfer(Transfer {
                job: 2,
                mode: TransferMode::Copy,
                state: TransferState::Asking,
                items: 6,
                name: "cover.png".into(),
                done_bytes: 1_800_000,
                total_bytes: 4_700_000,
                clash: Some(Clash {
                    id: 1,
                    name: "cover.png".into(),
                    folder: false,
                    original: ClashItem { size: Some(1_800_000), items: None, modified_ms: Some(1_791_800_000_000) },
                    replacement: ClashItem { size: Some(2_400_000), items: None, modified_ms: Some(1_793_640_000_000) },
                    others: 2,
                }),
                error: None,
            })),
            encode(Request {
                id: Some(102),
                command: Command::FilesTrash { paths: vec!["/home/user/Pictures/cover.png".into()], continues: true },
            }),
            encode(Request { id: Some(103), command: Command::FilesUndo { job: 3 } }),
            encode(Response { id: 103, outcome: Outcome::Result(json!(true)) }),
            encode(Request { id: Some(104), command: Command::FilesRedo { job: 4 } }),
            encode(Event::FilesHistory { undo: Some(FileStep::Move), redo: Some(FileStep::Trash) }),
            encode(Event::FilesHistory { undo: Some(FileStep::Copy), redo: None }),
            encode(Request { id: Some(81), command: Command::FilesSearch { query: "crimson".into() } }),
            encode(Response {
                id: 81,
                outcome: Outcome::Result(
                    serde_json::to_value(FileSearch {
                        ready: true,
                        total: 1,
                        results: vec![FoundFile {
                            folder: "/home/user/.local/share/Steam/steamapps/common".into(),
                            entry: FileEntry {
                                name: "Crimson Desert".into(),
                                dir: true,
                                link: false,
                                hidden: false,
                                archive: false,
                                mime: "inode/directory".into(),
                                size: None,
                                modified_ms: Some(1_793_000_000_000),
                            },
                            in_hidden_folder: true,
                            marks: vec![TextMark { start: 0, length: 7 }],
                        }],
                    })
                    .unwrap(),
                ),
            }),
            encode(Request { id: Some(82), command: Command::FilesProperties { path: "/home/user/notes.txt".into() } }),
            encode(Response {
                id: 82,
                outcome: Outcome::Result(
                    serde_json::to_value(FileProperties {
                        folder: "/home/user".into(),
                        entry: FileEntry {
                            name: "notes.txt".into(),
                            dir: false,
                            link: false,
                            hidden: false,
                            archive: false,
                            mime: "text/plain".into(),
                            size: Some(12),
                            modified_ms: Some(1_793_000_000_000),
                        },
                        link_target: None,
                        disk_bytes: Some(4096),
                        created_ms: Some(1_792_000_000_000),
                        accessed_ms: Some(1_793_500_000_000),
                        owner: "user".into(),
                        group: "users".into(),
                        access: Access {
                            owner: Rights { read: true, write: true, run: false },
                            group: Rights { read: true, write: false, run: false },
                            others: Rights::default(),
                        },
                        yours: true,
                        drive: Some(DriveSpace { size: 500_000_000_000, used: 120_000_000_000 }),
                    })
                    .unwrap(),
                ),
            }),
            encode(Request {
                id: Some(83),
                command: Command::FilesMeasure { job: 4, paths: vec!["/home/user/Pictures".into()] },
            }),
            encode(Request { id: Some(84), command: Command::FilesMeasureCancel { job: 4 } }),
            encode(Event::FilesMeasured(Measured {
                job: 4,
                files: 120,
                folders: 14,
                bytes: 1_400_000_000,
                disk_bytes: 1_400_800_000,
                done: false,
            })),
            encode(Request {
                id: Some(85),
                command: Command::FilesSetAccess { path: "/home/user/run.sh".into(), access: Access::default() },
            }),
            encode(Response { id: 8, outcome: Outcome::Error("bad request".into()) }),
        ];
        let all: String = lines.into_iter().map(Result::unwrap).collect();
        assert!(all.lines().all(|l| l.starts_with(r#"{"v":2,"#)));
        insta::assert_snapshot!(all);
    }

    fn desktop() -> Desktop {
        let workspace = |id, name: &str, active, focused| Workspace {
            id,
            name: name.into(),
            active,
            focused,
            urgent: false,
            empty: false,
            fullscreen: false,
            special: false,
            shown_elsewhere: false,
            active_window: None,
        };
        Desktop {
            compositor: "niri".into(),
            capabilities: vec![Capability::Workspaces, Capability::FocusedWindow, Capability::Fullscreen],
            outputs: vec![
                Output {
                    name: "DP-1".into(),
                    focused: true,
                    workspaces: vec![
                        Workspace { empty: true, ..workspace(1, "1", false, false) },
                        Workspace { active_window: Some("7".into()), ..workspace(2, "web", true, true) },
                    ],
                    layout: 3,
                },
                Output {
                    name: "HDMI-A-1".into(),
                    focused: false,
                    workspaces: vec![Workspace { urgent: true, fullscreen: true, ..workspace(3, "1", true, false) }],
                    layout: 0,
                },
            ],
            focused_window: Some(FocusedWindow { id: "7".into(), app_id: "foot".into(), title: "~".into() }),
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

    fn host() -> Host {
        Host {
            host: "RyzenShine".into(),
            user: "Luna".into(),
            booted_at: 1_793_000_000,
            os: OperatingSystem {
                id: "nixos".into(),
                id_like: vec![],
                name: "NixOS".into(),
                logo: Some("nix-snowflake".into()),
            },
        }
    }

    fn resources() -> Resources {
        Resources {
            cpu: 23.5,
            memory_used: 10_093_173_760,
            memory_total: 34_359_738_368,
            gpu: Some(18.0),
            temperature: None,
            history: vec![ResourceSample { cpu: 21.0, memory: 29.0 }, ResourceSample { cpu: 23.5, memory: 29.4 }],
            history_size: 36,
            apps: vec![AppUsage {
                name: "firefox".into(),
                key: "/usr/lib/firefox/firefox".into(),
                processes: 9,
                cpu: 6.1,
                memory: 2_400_000_000,
                protected: false,
            }],
        }
    }

    fn launcher() -> Launcher {
        Launcher {
            apps: vec![
                App {
                    id: "org.mozilla.firefox.desktop".into(),
                    name: "Firefox".into(),
                    description: Some("Web Browser".into()),
                    icon: Some("firefox".into()),
                },
                App { id: "foot.desktop".into(), name: "Foot".into(), description: None, icon: None },
            ],
            pinned: vec!["foot.desktop".into()],
            recent: vec!["org.mozilla.firefox.desktop".into(), "foot.desktop".into()],
        }
    }

    fn network() -> Network {
        Network {
            running: true,
            primary: Some(LinkKind::Wifi),
            prefer: Some(LinkKind::Wifi),
            wired: vec![WiredLink {
                interface: "enp5s0".into(),
                state: LinkState::Connected,
                connection: Some("Wired connection 1".into()),
                speed_mbps: Some(1000),
            }],
            wifi: Some(Wifi {
                interface: "wlp6s0".into(),
                enabled: true,
                blocked: false,
                state: LinkState::Connected,
                network: Some("Home".into()),
                speed_mbps: Some(585),
                scanning: false,
                networks: vec![
                    WifiNetwork {
                        ssid: "Home".into(),
                        strength: 84,
                        security: WifiSecurity::Wpa2,
                        band: Some(WifiBand::Ghz2_4),
                        saved: true,
                        connected: true,
                    },
                    WifiNetwork {
                        ssid: "Cafe".into(),
                        strength: 31,
                        security: WifiSecurity::Open,
                        band: Some(WifiBand::Ghz5),
                        saved: false,
                        connected: false,
                    },
                ],
                attempt: Some(WifiAttempt { ssid: "Home".into(), outcome: AttemptOutcome::Connected }),
            }),
        }
    }

    fn bluetooth() -> Bluetooth {
        Bluetooth {
            radio: BluetoothRadio::On,
            scanning: true,
            devices: vec![
                BluetoothDevice {
                    address: "00:1B:66:AA:BB:CC".into(),
                    name: "WH-1000XM5".into(),
                    kind: BluetoothKind::Headphones,
                    status: BluetoothStatus::Connected,
                    battery: Some(80),
                    error: None,
                },
                BluetoothDevice {
                    address: "9C:2A:70:11:22:33".into(),
                    name: "Pixel 8".into(),
                    kind: BluetoothKind::Phone,
                    status: BluetoothStatus::Pairing,
                    battery: None,
                    error: None,
                },
                BluetoothDevice {
                    address: "C4:30:18:44:55:66".into(),
                    name: "JBL Flip 6".into(),
                    kind: BluetoothKind::Speaker,
                    status: BluetoothStatus::Nearby,
                    battery: None,
                    error: Some("Couldn't reach it. Make sure it's on and nearby.".into()),
                },
            ],
            pairing: Some(PairingRequest {
                id: 3,
                address: "9C:2A:70:11:22:33".into(),
                ask: PairingAsk::Confirm { code: "482913".into() },
                incoming: false,
            }),
        }
    }

    #[test]
    fn network_rejects_unknown_fields() {
        let network = serde_json::to_value(network()).unwrap();
        assert!(serde_json::from_value::<Network>(network.clone()).is_ok());
        let mut extra = network.clone();
        extra["wifi"]["networks"][0]["bssid"] = json!("02:00:00:00:00:08");
        assert!(serde_json::from_value::<Network>(extra).is_err());
    }

    #[test]
    fn wifi_passwords_stay_out_of_debug_output() {
        let command = Command::ConnectWifi { ssid: "Home".into(), password: Some(WifiPassword::new("hunter22")) };
        assert!(!format!("{command:?}").contains("hunter22"));
        let line = encode(Request { id: None, command }).unwrap();
        assert!(line.contains(r#""password":"hunter22""#), "{line}");
    }

    fn peripherals() -> Peripherals {
        Peripherals {
            devices: vec![
                Peripheral {
                    id: "upower/mouse_hidpp_battery_0".into(),
                    name: "PRO X Wireless".into(),
                    kind: PeripheralKind::Mouse,
                    link: Some(PeripheralLink::Dongle),
                    battery: Some(Battery::Percent(72)),
                    charging: false,
                    source: PeripheralSource::Upower,
                },
                Peripheral {
                    id: "upower/keyboard_dev_AA_BB_CC_DD_EE_FF".into(),
                    name: "K380".into(),
                    kind: PeripheralKind::Keyboard,
                    link: Some(PeripheralLink::Bluetooth),
                    battery: Some(Battery::Level(BatteryLevel::High)),
                    charging: false,
                    source: PeripheralSource::Upower,
                },
                Peripheral {
                    id: "keychron/model-3".into(),
                    name: "Keychron K2 HE".into(),
                    kind: PeripheralKind::Keyboard,
                    link: Some(PeripheralLink::Dongle),
                    battery: Some(Battery::Percent(78)),
                    charging: false,
                    source: PeripheralSource::Keychron,
                },
                Peripheral {
                    id: "headsetcontrol/1038:2253".into(),
                    name: "SteelSeries Arctis Nova 5X".into(),
                    kind: PeripheralKind::Headset,
                    link: Some(PeripheralLink::Dongle),
                    battery: Some(Battery::Percent(30)),
                    charging: true,
                    source: PeripheralSource::Headsetcontrol,
                },
            ],
        }
    }

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

    fn tray() -> Tray {
        Tray {
            items: vec![
                TrayItem {
                    address: ":1.421/StatusNotifierItem".into(),
                    id: "discord_status_icon_1".into(),
                    title: String::new(),
                    app: Some(App {
                        id: "discord.desktop".into(),
                        name: "Discord".into(),
                        description: Some("All-in-one cross-platform voice and text chat for gamers".into()),
                        icon: Some("discord".into()),
                    }),
                    menu_only: false,
                },
                TrayItem {
                    address: ":1.607/org/ayatana/NotificationItem/steam".into(),
                    id: "steam".into(),
                    title: "Steam".into(),
                    app: None,
                    menu_only: true,
                },
            ],
            hidden: vec!["steam".into()],
        }
    }

    fn notifications() -> NotificationList {
        NotificationList {
            items: vec![
                Notification {
                    id: 7,
                    app: "Firefox".into(),
                    icon: Some("firefox".into()),
                    app_id: Some("org.mozilla.firefox.desktop".into()),
                    summary: "Download complete".into(),
                    body: "niri-26.04.tar.gz".into(),
                    image: None,
                    actions: vec![NotificationAction { key: "open".into(), label: "Open file".into() }],
                    has_default: true,
                    urgency: Urgency::Normal,
                    progress: None,
                    timeout_ms: None,
                    time: 1_761_400_000,
                    unread: true,
                    live: true,
                    device: None,
                },
                Notification {
                    id: 3,
                    app: "Discord".into(),
                    icon: None,
                    app_id: None,
                    summary: "Mara".into(),
                    body: "sent you the map".into(),
                    image: Some("/home/mara/.local/state/nova/notifications/3.png".into()),
                    actions: vec![],
                    has_default: false,
                    urgency: Urgency::Critical,
                    progress: Some(40),
                    timeout_ms: Some(0),
                    time: 1_761_399_000,
                    unread: false,
                    live: false,
                    device: Some(NotificationDevice {
                        kind: PeripheralKind::Mouse,
                        battery: Battery::Level(BatteryLevel::Critical),
                    }),
                },
            ],
            do_not_disturb: false,
            muted: vec!["Discord".into()],
            apps: vec![
                NotificationApp {
                    name: "Firefox".into(),
                    icon: Some("firefox".into()),
                    app_id: Some("org.mozilla.firefox.desktop".into()),
                },
                NotificationApp { name: "Discord".into(), icon: None, app_id: None },
            ],
            transient: vec![],
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
            "bg": "#1D1D1D", "bgAlt": "#393E46", "backdrop": "#000000", "glassTint": "#0A0807", "fg": "#FFF6EB",
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
            features: vec![FeatureState {
                id: "bar".into(),
                status: FeatureStatus::Ready,
                missing: vec![],
                slots: vec![],
            }],
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
        assert!(
            "calculator"
                .parse::<Panel>()
                .unwrap_err()
                .contains("known: ai-usage, power, quick-settings, launcher, tray, media, wallpaper")
        );
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
        assert_eq!(Event::Lock { locked: true }.topic(), Some(Topic::Lock));
        assert_eq!(Event::Volume { key: VolumeKey::Up }.topic(), Some(Topic::Volume));
        assert_eq!(Event::CaptureEnded.topic(), Some(Topic::Screenshot));
        assert_eq!(Event::Recording(None).topic(), Some(Topic::Recording));
        assert_eq!(Event::Host(host()).topic(), Some(Topic::Host));
        assert_eq!(Event::Resources(resources()).topic(), Some(Topic::Resources));
        assert_eq!(Event::Launcher(launcher()).topic(), Some(Topic::Launcher));
        assert_eq!(Event::Tray(tray()).topic(), Some(Topic::Tray));
        assert_eq!(Event::Notifications(notifications()).topic(), Some(Topic::Notifications));
        assert_eq!(Event::NotificationPopup(notifications().items[0].clone()).topic(), Some(Topic::Notifications));
        assert_eq!(Event::Media(media()).topic(), Some(Topic::Media));
        assert_eq!(Event::Peripherals(peripherals()).topic(), Some(Topic::Peripherals));
        assert_eq!(Event::Network(network()).topic(), Some(Topic::Network));
        let traffic = NetworkTraffic { down: vec![], up: vec![], today_down: 0, today_up: 0 };
        assert_eq!(Event::NetworkTraffic(traffic).topic(), Some(Topic::Network));
        assert_eq!(Event::Bluetooth(bluetooth()).topic(), Some(Topic::Bluetooth));
    }

    #[test]
    fn bluetooth_rejects_unknown_fields() {
        let state = serde_json::to_value(bluetooth()).unwrap();
        assert!(serde_json::from_value::<Bluetooth>(state.clone()).is_ok());
        for pointer in ["", "/devices/0", "/pairing", "/pairing/ask"] {
            let mut extra = state.clone();
            extra.pointer_mut(pointer).unwrap().as_object_mut().unwrap().insert("extra".into(), 1.into());
            assert!(serde_json::from_value::<Bluetooth>(extra).is_err(), "{pointer}");
        }
        let answer = |value| serde_json::from_value::<PairingAnswer>(value).ok();
        assert_eq!(answer(json!("accept")), Some(PairingAnswer::Accept));
        assert_eq!(answer(json!({"code": "0000"})), Some(PairingAnswer::Code("0000".into())));
        assert_eq!(answer(json!({"code": "0000", "extra": 1})), None);
        assert_eq!(answer(json!("maybe")), None);
    }

    #[test]
    fn peripherals_reject_unknown_fields() {
        let list = serde_json::to_value(peripherals()).unwrap();
        assert!(serde_json::from_value::<Peripherals>(list.clone()).is_ok());
        for pointer in ["", "/devices/0"] {
            let mut extra = list.clone();
            extra.pointer_mut(pointer).unwrap().as_object_mut().unwrap().insert("extra".into(), 1.into());
            assert!(serde_json::from_value::<Peripherals>(extra).is_err(), "{pointer}");
        }
        let mut odd = list.clone();
        odd["devices"][0]["battery"] = json!({"percent": 30, "level": "low"});
        assert!(serde_json::from_value::<Peripherals>(odd).is_err());
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
        assert!(decode_request(seek).unwrap_err().contains("unknown field"));
        let control = r#"{"v":2,"cmd":"media_control","args":{"player":"p","action":"shuffle"}}"#;
        assert!(decode_request(control).is_err());
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
        let rect = r#"{"x":0,"y":0,"width":1,"height":1,"scale":2}"#;
        assert!(
            decode_request(&format!(r#"{{"v":2,"cmd":"save_capture","args":{{"output":"DP-1","rect":{rect}}}}}"#))
                .is_err()
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
    fn power_menu_rejects_unknown_fields() {
        let menu = r#"{"options":[{"action":"lock","available":true}],"booted_at":1}"#;
        assert!(serde_json::from_str::<PowerMenu>(menu).is_ok());
        assert!(serde_json::from_str::<PowerMenu>(r#"{"options":[],"extra":1}"#).is_err());
        let option = r#"{"options":[{"action":"lock","available":true,"extra":1}]}"#;
        assert!(serde_json::from_str::<PowerMenu>(option).is_err());
    }

    #[test]
    fn host_and_resources_reject_unknown_fields() {
        fn with_extra(value: serde_json::Value, pointer: &str) -> serde_json::Value {
            let mut value = value;
            value.pointer_mut(pointer).unwrap().as_object_mut().unwrap().insert("extra".into(), 1.into());
            value
        }
        let host = serde_json::to_value(host()).unwrap();
        assert!(serde_json::from_value::<Host>(host.clone()).is_ok());
        for pointer in ["", "/os"] {
            assert!(serde_json::from_value::<Host>(with_extra(host.clone(), pointer)).is_err(), "{pointer}");
        }
        let resources = serde_json::to_value(resources()).unwrap();
        assert!(serde_json::from_value::<Resources>(resources.clone()).is_ok());
        for pointer in ["", "/history/0", "/apps/0"] {
            assert!(serde_json::from_value::<Resources>(with_extra(resources.clone(), pointer)).is_err(), "{pointer}");
        }
    }

    #[test]
    fn notifications_reject_unknown_fields() {
        let list = serde_json::to_value(notifications()).unwrap();
        assert!(serde_json::from_value::<NotificationList>(list.clone()).is_ok());
        for pointer in ["", "/items/0", "/items/0/actions/0", "/apps/0"] {
            let mut extra = list.clone();
            extra.pointer_mut(pointer).unwrap().as_object_mut().unwrap().insert("extra".into(), 1.into());
            assert!(serde_json::from_value::<NotificationList>(extra).is_err(), "{pointer}");
        }
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
