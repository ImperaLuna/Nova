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

// The envelope and the log feed are the framework's (`nova-carrier`), which
// carries these messages without knowing them; re-exported so clients name
// them from here as before.
pub use nova_carrier::envelope::REQUEST_MAX_BYTES;
pub use nova_carrier::log::{LogEntry, LogLevel, Logs, UI_LOG_SOURCE};
pub use nova_carrier::{Envelope, Outcome, PROTOCOL_VERSION, Response, encode};
// The shared lookups' own types are the framework's (IMP-374), re-exported
// so clients name them from here as before.
pub use nova_apps::{APP_ICON_MAX_CHARS, App};
pub use nova_compositor::{Capability, Desktop, FocusedWindow, Output, Step, Workspace, WorkspaceId};
pub use nova_host::{Host, OperatingSystem};
pub use nova_theme::{Color, Palette, Theme, ThemeMode};

#[cfg(test)]
mod js;

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
    Logs(Logs),
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
}

impl Event {
    /// The topic a client must subscribe to for this event, `None` for events
    /// every client gets.
    pub fn topic(&self) -> Option<Topic> {
        match self {
            Event::Hello { .. } => None,
            Event::Features { .. } => Some(Topic::Features),
            Event::Logs(_) => Some(Topic::Logs),
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

/// The launcher provider that finds files (feature `nova-files`): the
/// launcher's Files mode, which searches the names the Nova Files app
/// indexes and opens or shows what it finds there.
pub const PROVIDER_FILES: &str = "files";

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
    /// What it is when it's a file or folder ([`PROVIDER_FILES`]); then
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
    /// blank query finds nothing. With a `mode` (a provider's view id, like
    /// [`PROVIDER_FILES`]) the launcher is in that provider's own mode, and
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
    /// ([`PROVIDER_FILES`]) opens in its default app and a folder in Nova
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
}

/// A stretch of text to highlight, in UTF-16 code units, what QML's
/// strings count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextMark {
    pub start: u32,
    pub length: u32,
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

/// A request for one of the commands above.
pub type Request = nova_carrier::Request<Command>;

/// Reads one request line from a client. Anything on the socket is
/// untrusted: unknown fields, unknown commands, bad arguments and other
/// protocol versions are all errors.
pub fn decode_request(line: &str) -> Result<Request, String> {
    nova_carrier::decode_request(line)
}

impl From<Topic> for nova_carrier::Topic {
    /// The topic by its wire name, which serde gives.
    fn from(topic: Topic) -> Self {
        nova_carrier::Topic::named(nova_carrier::names::names_of::<Topic>()[topic as usize])
    }
}

impl nova_carrier::Published for Event {
    fn topic(&self) -> Option<nova_carrier::Topic> {
        Event::topic(self).map(Into::into)
    }
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
                        mode: None,
                    },
                    FeatureState {
                        id: "nova-files".into(),
                        status: FeatureStatus::Ready,
                        missing: vec![],
                        slots: vec![Slot { host: "launcher.providers".into(), view: "files".into(), order: 20 }],
                        mode: Some("full".into()),
                    },
                    FeatureState {
                        id: "recording".into(),
                        status: FeatureStatus::MissingDeps,
                        missing: vec!["gpu-screen-recorder|wf-recorder".into()],
                        slots: vec![],
                        mode: None,
                    },
                    FeatureState {
                        id: "weather".into(),
                        status: FeatureStatus::Off,
                        missing: vec![],
                        slots: vec![],
                        mode: None,
                    },
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
                    ],
                },
            }),
            encode(Event::Theme(theme())),
            encode(Request { id: Some(10), command: Command::Status }),
            encode(Request { id: Some(11), command: Command::Quit }),
            encode(Event::Logs(Logs { backlog: true, entries: vec![log_entry()] })),
            encode(Event::Logs(Logs { backlog: false, entries: vec![log_entry()] })),
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
            encode(Request { id: Some(24), command: Command::LauncherSearch { query: "fi".into(), mode: None } }),
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
                        file: None,
                    }])
                    .unwrap(),
                ),
            }),
            encode(Request {
                id: Some(91),
                command: Command::LauncherSearch { query: "~/Doc".into(), mode: Some(PROVIDER_FILES.into()) },
            }),
            encode(Response {
                id: 91,
                outcome: Outcome::Result(
                    serde_json::to_value(vec![LauncherItem {
                        provider: PROVIDER_FILES.into(),
                        id: "/home/me/Documents".into(),
                        name: vec![
                            TextRun { text: "Doc".into(), matched: true },
                            TextRun { text: "uments".into(), matched: false },
                        ],
                        description: Some("~".into()),
                        icon: None,
                        file: Some(LauncherFile {
                            dir: true,
                            mime: "inode/directory".into(),
                            hidden: false,
                            completion: "~/Documents/".into(),
                        }),
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
                mode: None,
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

    /// The framework's log feed and this crate's `logs` event and topic
    /// are one thing on the wire.
    #[test]
    fn the_log_feed_is_the_logs_event() {
        let logs = Logs { backlog: true, entries: vec![log_entry()] };
        assert_eq!(encode(logs.event()).unwrap(), encode(Event::Logs(logs)).unwrap());
        assert_eq!(nova_carrier::Topic::from(Topic::Logs), nova_carrier::log::TOPIC);
    }

    /// The framework's compositor event and this crate's `compositor` event
    /// and topic are one thing on the wire.
    #[test]
    fn the_desktop_is_the_compositor_event() {
        let desktop = desktop();
        assert_eq!(encode(nova_compositor::event(&desktop)).unwrap(), encode(Event::Compositor(desktop)).unwrap());
        assert_eq!(nova_carrier::Topic::from(Topic::Compositor), nova_compositor::TOPIC);
    }

    /// Every topic's framework name is its wire name.
    #[test]
    fn topics_keep_their_wire_names() {
        for (index, name) in nova_carrier::names::names_of::<Topic>().iter().enumerate() {
            let topic: Topic = serde_json::from_value(json!(name)).unwrap();
            assert_eq!(topic as usize, index);
            assert_eq!(nova_carrier::Topic::from(topic).name(), *name);
        }
    }

    #[test]
    fn only_hello_has_no_topic() {
        assert_eq!(Event::Hello { daemon: "x".into() }.topic(), None);
        assert_eq!(Event::Features { features: vec![] }.topic(), Some(Topic::Features));
        assert_eq!(Event::Logs(Logs { backlog: false, entries: vec![] }).topic(), Some(Topic::Logs));
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
