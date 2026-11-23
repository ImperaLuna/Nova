//! The wire format: every message shape novad and its clients exchange, as
//! one snapshot, and the rules every request line is read by. The types are
//! the ones novad serves: its own (`nova-daemon-info`), the -info crates'
//! and each feature's api crate's.

use nova_app_info::App;
use nova_bar_api::Panel;
use nova_carrier::log::{LogEntry, LogLevel, Logs, UI_LOG_SOURCE};
use nova_carrier::{Envelope, Outcome, Published, Request, Response, encode};
use nova_daemon_info::{
    Command, Crash, Event, FeatureState, FeatureStatus, Slot, Status, UiProgram, UiState, UiStatus, topics,
};
use nova_desktop_info::{Capability, FocusedWindow, Output, Workspace};
use nova_desktop_info::{Desktop, Step};
use nova_host_info::Host;
use nova_host_info::OperatingSystem;
use nova_media_api::Command as MediaCommand;
use nova_notifications_api::Command as NotificationCommand;
use nova_power_api::{PowerAction, PowerMenu, PowerOption};
use nova_recording_api::{ActiveRecording, LayoutRect, RecordAudio, SavedRecording};
use nova_screenshot_api::{CaptureFrame, CaptureMode, PixelRect, SavedScreenshot, ScreenshotAction};
use nova_theme_info::Theme;
use nova_theme_info::{Color, Palette, ThemeMode};
use nova_volume_osd_api::VolumeKey;
use serde_json::json;

/// Every message shape on the wire. A change here changes the protocol:
/// accept with `cargo insta review`, then run `just protocol-js`.
mod samples;
use samples::*;

#[test]
fn wire_format() {
    let lines = [
        own_lines(),
        ai_usage_lines(),
        lock_lines(),
        power_lines(),
        screenshot_lines(),
        recording_lines(),
        resources_lines(),
        launcher_lines(),
        tray_lines(),
        notifications_lines(),
        wallpaper_lines(),
        media_lines(),
        peripherals_lines(),
        network_lines(),
        bluetooth_lines(),
        sound_lines(),
    ];
    let all: String = lines.into_iter().flatten().map(Result::unwrap).collect();
    assert!(all.lines().all(|l| l.starts_with(r#"{"v":2,"#)));
    // The snapshot keeps the name and folder-relative file it had in
    // nova-protocol, so the file is unchanged by the move.
    insta::with_settings!({ prepend_module_to_snapshot => false }, {
        insta::assert_snapshot!("nova_protocol__tests__wire_format", all);
    });
}

/// novad's own messages, the -info crates' and the shared envelope's.
fn own_lines() -> Vec<serde_json::Result<String>> {
    vec![
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
                topics: [
                    topics::FEATURES,
                    topics::LOGS,
                    nova_theme_info::TOPIC,
                    nova_ai_usage_api::TOPIC,
                    nova_desktop_info::TOPIC,
                    nova_bar_api::TOPIC,
                    nova_lock_api::TOPIC,
                    nova_volume_osd_api::TOPIC,
                    nova_screenshot_api::TOPIC,
                    nova_recording_api::TOPIC,
                    nova_host_info::TOPIC,
                    nova_resources_api::TOPIC,
                    nova_launcher_api::TOPIC,
                    nova_tray_api::TOPIC,
                    nova_notifications_api::TOPIC,
                    nova_media_api::TOPIC,
                    nova_peripherals_api::TOPIC,
                    nova_network_api::TOPIC,
                    nova_bluetooth_api::TOPIC,
                    nova_sound_api::TOPIC,
                ]
                .map(|topic| topic.name().to_string())
                .to_vec(),
            },
        }),
        encode(nova_theme_info::Event::Theme(theme())),
        encode(Request { id: Some(10), command: Command::Status }),
        encode(Request { id: Some(11), command: Command::Quit }),
        encode(Event::Logs(Logs { backlog: true, entries: vec![log_entry()] })),
        encode(Event::Logs(Logs { backlog: false, entries: vec![log_entry()] })),
        encode(Response { id: 10, outcome: Outcome::Result(serde_json::to_value(status()).unwrap()) }),
        encode(Response { id: 7, outcome: Outcome::Result(json!("pong")) }),
    ]
}

/// The AI usage feature's own messages (`nova-ai-usage-api`), and what follows them.
fn ai_usage_lines() -> Vec<serde_json::Result<String>> {
    vec![
        encode(nova_ai_usage_api::Event::AiUsage(ai_usage())),
        encode(nova_carrier::Request {
            id: Some(12),
            command: nova_ai_usage_api::Command::ShowAiUsage { provider: nova_ai_usage_api::AiProvider::Codex },
        }),
        encode(nova_carrier::Request { id: None, command: nova_ai_usage_api::Command::RefreshAiUsage }),
        encode(nova_carrier::Request {
            id: Some(13),
            command: nova_ai_usage_api::Command::ReportClaudeUsage { limits: ai_usage().providers[0].limits.clone() },
        }),
        encode(nova_desktop_info::Event::Compositor(desktop())),
        encode(Request { id: Some(14), command: nova_workspaces_api::Command::FocusWorkspace { id: -1337 } }),
        encode(Request {
            id: None,
            command: nova_workspaces_api::Command::StepWorkspace { output: "DP-1".into(), step: Step::Previous },
        }),
        encode(Request { id: Some(15), command: nova_bar_api::Command::Toggle { panel: Panel::AiUsage } }),
        encode(nova_bar_api::Event::Toggle { panel: Panel::AiUsage, output: Some("DP-1".into()) }),
        encode(nova_bar_api::Event::Toggle { panel: Panel::AiUsage, output: None }),
    ]
}

/// The lock feature's own messages (`nova-lock-api`), and what follows them.
fn lock_lines() -> Vec<serde_json::Result<String>> {
    vec![
        encode(nova_carrier::Request { id: Some(16), command: nova_lock_api::Command::Lock }),
        encode(nova_lock_api::Event::Lock { locked: true }),
        encode(nova_carrier::Request { id: Some(20), command: nova_lock_api::Command::ReportLocked }),
        encode(nova_carrier::Request { id: Some(17), command: nova_lock_api::Command::ReportUnlocked }),
        encode(nova_lock_api::Event::Lock { locked: false }),
    ]
}

/// The power feature's own messages (`nova-power-api`), and what follows them.
fn power_lines() -> Vec<serde_json::Result<String>> {
    vec![
        encode(nova_carrier::Request { id: Some(18), command: nova_power_api::Command::PowerOptions }),
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
        encode(nova_carrier::Request {
            id: Some(19),
            command: nova_power_api::Command::Power { action: PowerAction::PowerOff },
        }),
        encode(Request { id: None, command: nova_bar_api::Command::Toggle { panel: Panel::Power } }),
        encode(Request { id: Some(21), command: nova_volume_osd_api::Command::Volume { key: VolumeKey::Up } }),
        encode(nova_volume_osd_api::Event::Volume { key: VolumeKey::Down }),
        encode(nova_volume_osd_api::Event::Volume { key: VolumeKey::Mute }),
    ]
}

/// The screenshot feature's own messages (`nova-screenshot-api`), and what follows them.
fn screenshot_lines() -> Vec<serde_json::Result<String>> {
    vec![
        encode(nova_carrier::Request {
            id: Some(30),
            command: nova_screenshot_api::Command::Screenshot { mode: CaptureMode::Region },
        }),
        encode(nova_carrier::Request {
            id: Some(31),
            command: nova_screenshot_api::Command::Screenshot { mode: CaptureMode::Screen },
        }),
        encode(nova_screenshot_api::Event::Capture {
            frames: vec![CaptureFrame { output: "DP-1".into(), path: "/run/user/1000/nova/capture/DP-1.ppm".into() }],
        }),
        encode(nova_carrier::Request {
            id: Some(32),
            command: nova_screenshot_api::Command::SaveCapture {
                output: "DP-1".into(),
                rect: PixelRect { x: 1460, y: 150, width: 960, height: 560 },
            },
        }),
        encode(nova_carrier::Request { id: Some(33), command: nova_screenshot_api::Command::CancelCapture }),
        encode(nova_screenshot_api::Event::CaptureEnded),
        encode(nova_screenshot_api::Event::ScreenshotSaved(SavedScreenshot {
            id: 1,
            path: "/home/me/Pictures/Screenshots/Screenshot 2026-10-25 15-03-41.png".into(),
            width: 960,
            height: 560,
            copied: true,
        })),
        encode(nova_carrier::Request {
            id: Some(34),
            command: nova_screenshot_api::Command::ScreenshotAction { id: 1, action: ScreenshotAction::Open },
        }),
        encode(nova_carrier::Request {
            id: None,
            command: nova_screenshot_api::Command::ScreenshotAction { id: 1, action: ScreenshotAction::Folder },
        }),
        encode(nova_carrier::Request {
            id: None,
            command: nova_screenshot_api::Command::ScreenshotAction { id: 1, action: ScreenshotAction::Delete },
        }),
    ]
}

/// The recording feature's own messages (`nova-recording-api`), and what follows them.
fn recording_lines() -> Vec<serde_json::Result<String>> {
    vec![
        encode(nova_carrier::Request { id: Some(35), command: nova_recording_api::Command::Record }),
        encode(nova_recording_api::Event::PickRecording),
        encode(nova_carrier::Request {
            id: Some(36),
            command: nova_recording_api::Command::StartRecording {
                output: "DP-1".into(),
                region: Some(LayoutRect { x: 2560, y: -40, width: 960, height: 540 }),
                audio: RecordAudio { desktop: true, mic: false },
            },
        }),
        encode(nova_recording_api::Event::Recording(Some(ActiveRecording {
            output: "DP-1".into(),
            region: Some(LayoutRect { x: 2560, y: -40, width: 960, height: 540 }),
            audio: RecordAudio { desktop: true, mic: false },
            elapsed_ms: 12_400,
            paused: false,
            can_pause: true,
        }))),
        encode(nova_carrier::Request {
            id: Some(37),
            command: nova_recording_api::Command::PauseRecording { paused: true },
        }),
        encode(nova_carrier::Request { id: Some(38), command: nova_recording_api::Command::StopRecording }),
        encode(nova_recording_api::Event::Recording(None)),
        encode(nova_recording_api::Event::RecordingSaved(SavedRecording {
            id: 1,
            path: "/home/me/Videos/Recordings/Recording 2026-10-25 18-03-41.mp4".into(),
            duration_ms: 42_000,
        })),
        encode(nova_recording_api::Event::RecordingFailed { reason: "wf-recorder wrote no frames".into() }),
        encode(nova_carrier::Request {
            id: None,
            command: nova_recording_api::Command::RecordingAction { id: 1, action: ScreenshotAction::Open },
        }),
        encode(nova_carrier::Request { id: None, command: nova_recording_api::Command::OpenLog }),
        encode(nova_host_info::Event::Host(host())),
    ]
}

/// The sound feature's own messages (`nova-sound-api`).
fn sound_lines() -> Vec<serde_json::Result<String>> {
    use nova_sound_api::{Command, Kind};
    vec![
        encode(nova_sound_api::Event::Sound(sound())),
        encode(nova_carrier::Request {
            id: Some(24),
            command: Command::SetSoundVolume { of: Kind::Output, id: 52, volume: 0.4 },
        }),
        encode(nova_carrier::Request {
            id: Some(25),
            command: Command::SetSoundMuted { of: Kind::App, id: 77, muted: true },
        }),
        encode(nova_carrier::Request {
            id: Some(28),
            command: Command::SetSoundLevel { of: Kind::Output, id: 52, volume: 0.7 },
        }),
        encode(nova_carrier::Request { id: Some(26), command: Command::UseSoundOutput { id: 61 } }),
        encode(nova_carrier::Request { id: Some(27), command: Command::UseSoundInput { id: 53 } }),
    ]
}

/// The resources feature's own messages (`nova-resources-api`), and what follows them.
fn resources_lines() -> Vec<serde_json::Result<String>> {
    vec![
        encode(nova_carrier::Request {
            id: Some(22),
            command: nova_resources_api::Command::WatchResources { watching: true },
        }),
        encode(nova_resources_api::Event::Resources(resources())),
        encode(nova_carrier::Request {
            id: Some(23),
            command: nova_resources_api::Command::EndApp { key: "/usr/lib/firefox/firefox".into() },
        }),
    ]
}

/// The quick settings toggle and the launcher's own messages (`nova-launcher-api`).
fn launcher_lines() -> Vec<serde_json::Result<String>> {
    vec![
        encode(Request { id: None, command: nova_bar_api::Command::Toggle { panel: Panel::QuickSettings } }),
        encode(nova_launcher_api::Event::Launcher(launcher())),
        encode(nova_carrier::Request {
            id: Some(24),
            command: nova_launcher_api::Command::LauncherSearch { query: "fi".into(), mode: None },
        }),
        encode(Response {
            id: 24,
            outcome: Outcome::Result(
                serde_json::to_value(vec![nova_launcher_api::LauncherItem {
                    provider: nova_launcher_api::PROVIDER_APPS.into(),
                    id: "org.mozilla.firefox.desktop".into(),
                    name: vec![
                        nova_launcher_api::TextRun { text: "Fi".into(), matched: true },
                        nova_launcher_api::TextRun { text: "refox".into(), matched: false },
                    ],
                    description: Some("Web Browser".into()),
                    icon: None,
                    file: None,
                }])
                .unwrap(),
            ),
        }),
        encode(nova_carrier::Request {
            id: Some(91),
            command: nova_launcher_api::Command::LauncherSearch {
                query: "~/Doc".into(),
                mode: Some(nova_nova_files_api::PROVIDER_FILES.into()),
            },
        }),
        encode(Response {
            id: 91,
            outcome: Outcome::Result(
                serde_json::to_value(vec![nova_launcher_api::LauncherItem {
                    provider: nova_nova_files_api::PROVIDER_FILES.into(),
                    id: "/home/me/Documents".into(),
                    name: vec![
                        nova_launcher_api::TextRun { text: "Doc".into(), matched: true },
                        nova_launcher_api::TextRun { text: "uments".into(), matched: false },
                    ],
                    description: Some("~".into()),
                    icon: None,
                    file: Some(nova_launcher_api::LauncherFile {
                        dir: true,
                        mime: "inode/directory".into(),
                        hidden: false,
                        completion: "~/Documents/".into(),
                    }),
                }])
                .unwrap(),
            ),
        }),
        encode(nova_carrier::Request {
            id: Some(44),
            command: nova_launcher_api::Command::AppIcon {
                icon_name: "".into(),
                program: "/nix/store/x-zen/lib/zen/.zen-wrapped".into(),
                name: "Zen".into(),
            },
        }),
        encode(Response { id: 44, outcome: Outcome::Result(json!("zen-browser")) }),
        encode(nova_carrier::Request {
            id: Some(25),
            command: nova_launcher_api::Command::LauncherRun {
                provider: nova_launcher_api::PROVIDER_APPS.into(),
                id: "foot.desktop".into(),
                output: Some("DP-1".into()),
                new_window: true,
            },
        }),
        encode(nova_carrier::Request {
            id: Some(26),
            command: nova_launcher_api::Command::PinApp { id: "foot.desktop".into(), pinned: true },
        }),
    ]
}

/// The tray feature's own messages (`nova-tray-api`), and what follows them.
fn tray_lines() -> Vec<serde_json::Result<String>> {
    vec![
        encode(nova_carrier::Request {
            id: Some(27),
            command: nova_tray_api::Command::HideTrayApp { id: "steam".into(), hidden: true },
        }),
        encode(nova_carrier::Request {
            id: Some(28),
            command: nova_tray_api::Command::ActivateTrayItem { address: ":1.42/StatusNotifierItem".into() },
        }),
        encode(nova_carrier::Request {
            id: Some(29),
            command: nova_tray_api::Command::TrayMenuPicked { address: ":1.42/StatusNotifierItem".into() },
        }),
        encode(nova_carrier::Request {
            id: Some(30),
            command: nova_tray_api::Command::SecondaryActivateTrayItem { address: ":1.42/StatusNotifierItem".into() },
        }),
        encode(nova_carrier::Request {
            id: Some(31),
            command: nova_tray_api::Command::TrayMenu { address: ":1.42/StatusNotifierItem".into(), id: 0 },
        }),
        encode(nova_carrier::Request {
            id: Some(32),
            command: nova_tray_api::Command::RunTrayMenuEntry {
                address: ":1.42/StatusNotifierItem".into(),
                id: 7,
                revision: 3,
            },
        }),
        encode(Response { id: 31, outcome: Outcome::Result(serde_json::to_value(tray_menu_level()).unwrap()) }),
        encode(nova_carrier::Request { id: None, command: nova_bar_api::Command::Toggle { panel: Panel::Launcher } }),
        encode(nova_tray_api::Event::Tray(tray())),
    ]
}

/// The notifications feature's own messages (`nova-notifications-api`), and what follows them.
fn notifications_lines() -> Vec<serde_json::Result<String>> {
    vec![
        encode(nova_notifications_api::Event::Notifications(notifications())),
        encode(nova_notifications_api::Event::NotificationPopup(notifications().items[0].clone())),
        encode(nova_carrier::Request {
            id: Some(40),
            command: NotificationCommand::NotificationAction { id: 7, action: "open".into() },
        }),
        encode(nova_carrier::Request {
            id: None,
            command: NotificationCommand::OpenNotification { id: 7, output: Some("DP-1".into()) },
        }),
        encode(nova_carrier::Request { id: Some(41), command: NotificationCommand::DismissNotification { id: 7 } }),
        encode(nova_carrier::Request {
            id: None,
            command: NotificationCommand::ClearNotifications { app: Some("Discord".into()) },
        }),
        encode(nova_carrier::Request { id: None, command: NotificationCommand::ClearNotifications { app: None } }),
        encode(nova_carrier::Request { id: None, command: NotificationCommand::ReadNotifications }),
        encode(nova_carrier::Request { id: Some(42), command: NotificationCommand::SetDoNotDisturb { on: true } }),
        encode(nova_carrier::Request {
            id: Some(43),
            command: NotificationCommand::MuteNotificationApp { app: "Discord".into(), muted: true },
        }),
    ]
}

/// The wallpaper feature's own messages (`nova-wallpaper-api`), and what follows them.
fn wallpaper_lines() -> Vec<serde_json::Result<String>> {
    vec![
        encode(nova_carrier::Request {
            id: Some(50),
            command: nova_wallpaper_api::Command::SetWallpaper {
                path: "/home/luna/Pictures/dusk.jpg".into(),
                output: None,
            },
        }),
        encode(nova_carrier::Request {
            id: Some(51),
            command: nova_wallpaper_api::Command::SetWallpaper {
                path: "/home/luna/Pictures/tide.jpg".into(),
                output: Some("DP-2".into()),
            },
        }),
        encode(nova_carrier::Request { id: Some(52), command: nova_wallpaper_api::Command::Wallpapers }),
        encode(Response {
            id: 52,
            outcome: Outcome::Result(
                serde_json::to_value(nova_wallpaper_api::Wallpapers {
                    path: Some("/home/luna/Pictures/dusk.jpg".into()),
                    outputs: [("DP-2".into(), "/home/luna/Pictures/tide.jpg".into())].into(),
                })
                .unwrap(),
            ),
        }),
        encode(nova_carrier::Request { id: Some(53), command: nova_wallpaper_api::Command::WallpaperFolder }),
        encode(Response {
            id: 53,
            outcome: Outcome::Result(
                serde_json::to_value(nova_wallpaper_api::WallpaperFolder {
                    folder: "/home/luna/Pictures/Wallpapers".into(),
                    images: vec![
                        "/home/luna/Pictures/Wallpapers/dusk.jpg".into(),
                        "/home/luna/Pictures/Wallpapers/tide.jpg".into(),
                    ],
                })
                .unwrap(),
            ),
        }),
        encode(Request { id: None, command: nova_bar_api::Command::Toggle { panel: Panel::Wallpaper } }),
    ]
}

/// The media feature's own messages (`nova-media-api`), and what follows them.
fn media_lines() -> Vec<serde_json::Result<String>> {
    vec![
        encode(nova_media_api::Event::Media(media())),
        encode(nova_carrier::Request {
            id: Some(45),
            command: MediaCommand::MediaControl {
                player: "org.mpris.MediaPlayer2.spotify".into(),
                action: nova_media_api::MediaAction::PlayPause,
            },
        }),
        encode(nova_carrier::Request {
            id: None,
            command: MediaCommand::MediaControl {
                player: "org.mpris.MediaPlayer2.spotify".into(),
                action: nova_media_api::MediaAction::Next,
            },
        }),
        encode(nova_carrier::Request {
            id: None,
            command: MediaCommand::MediaControl {
                player: "org.mpris.MediaPlayer2.spotify".into(),
                action: nova_media_api::MediaAction::Previous,
            },
        }),
        encode(nova_carrier::Request {
            id: Some(46),
            command: MediaCommand::MediaSeek {
                player: "org.mpris.MediaPlayer2.spotify".into(),
                position_us: 84_000_000,
            },
        }),
        encode(nova_carrier::Request {
            id: Some(47),
            command: MediaCommand::MediaShow { player: "org.mpris.MediaPlayer2.firefox.instance_1_42".into() },
        }),
        encode(nova_carrier::Request {
            id: Some(48),
            command: MediaCommand::MediaFocus { player: "org.mpris.MediaPlayer2.spotify".into() },
        }),
        encode(Response { id: 48, outcome: Outcome::Result(json!(true)) }),
        encode(nova_carrier::Request {
            id: Some(49),
            command: MediaCommand::MediaWindow { player: "org.mpris.MediaPlayer2.spotify".into() },
        }),
        encode(Response { id: 49, outcome: Outcome::Result(json!("3")) }),
    ]
}

/// The peripherals feature's own messages (`nova-peripherals-api`), and what follows them.
fn peripherals_lines() -> Vec<serde_json::Result<String>> {
    vec![
        encode(nova_peripherals_api::Event::Peripherals(peripherals())),
        encode(nova_peripherals_api::Event::Peripherals(nova_peripherals_api::Peripherals { devices: vec![] })),
    ]
}

/// The network feature's own messages (`nova-network-api`), and what follows them.
fn network_lines() -> Vec<serde_json::Result<String>> {
    vec![
        encode(nova_network_api::Event::Network(network())),
        encode(nova_network_api::Event::Network(nova_network_api::Network {
            running: false,
            primary: None,
            prefer: None,
            wired: vec![],
            wifi: None,
        })),
        encode(nova_carrier::Request { id: Some(50), command: nova_network_api::Command::SetWifi { enabled: false } }),
        encode(nova_carrier::Request { id: Some(51), command: nova_network_api::Command::ScanWifi }),
        encode(nova_carrier::Request {
            id: Some(52),
            command: nova_network_api::Command::ConnectWifi {
                ssid: "Home".into(),
                password: Some(nova_network_api::WifiPassword::new("correct horse")),
            },
        }),
        encode(nova_carrier::Request {
            id: Some(53),
            command: nova_network_api::Command::ConnectWifi { ssid: "Cafe".into(), password: None },
        }),
        encode(nova_carrier::Request {
            id: Some(54),
            command: nova_network_api::Command::PreferNetwork { kind: nova_network_api::LinkKind::Wired },
        }),
        encode(nova_carrier::Request { id: Some(56), command: nova_network_api::Command::DisconnectWifi }),
        encode(nova_carrier::Request {
            id: Some(57),
            command: nova_network_api::Command::ForgetWifi { ssid: "Cafe".into() },
        }),
        encode(nova_carrier::Request {
            id: Some(55),
            command: nova_network_api::Command::WatchNetworkTraffic { watching: true },
        }),
        encode(nova_network_api::Event::NetworkTraffic(nova_network_api::NetworkTraffic {
            down: vec![0, 1_250_000, 3_400_000],
            up: vec![0, 42_000, 61_000],
            today_down: 4_200_000_000,
            today_up: 310_000_000,
        })),
    ]
}

/// The Bluetooth feature's own messages (`nova-bluetooth-api`), and what follows them.
fn bluetooth_lines() -> Vec<serde_json::Result<String>> {
    vec![
        encode(nova_bluetooth_api::Event::Bluetooth(bluetooth())),
        encode(nova_carrier::Request {
            id: Some(60),
            command: nova_bluetooth_api::Command::BluetoothPower { on: true },
        }),
        encode(nova_carrier::Request {
            id: Some(61),
            command: nova_bluetooth_api::Command::WatchBluetooth { watching: true },
        }),
        encode(nova_carrier::Request {
            id: Some(62),
            command: nova_bluetooth_api::Command::BluetoothConnect { address: "9C:2A:70:11:22:33".into() },
        }),
        encode(nova_carrier::Request {
            id: Some(63),
            command: nova_bluetooth_api::Command::BluetoothDisconnect { address: "00:1B:66:AA:BB:CC".into() },
        }),
        encode(nova_carrier::Request {
            id: Some(64),
            command: nova_bluetooth_api::Command::BluetoothForget { address: "00:1B:66:AA:BB:CC".into() },
        }),
        encode(nova_carrier::Request {
            id: Some(65),
            command: nova_bluetooth_api::Command::BluetoothAnswer {
                id: 3,
                answer: nova_bluetooth_api::PairingAnswer::Accept,
            },
        }),
        encode(nova_carrier::Request {
            id: Some(66),
            command: nova_bluetooth_api::Command::BluetoothAnswer {
                id: 4,
                answer: nova_bluetooth_api::PairingAnswer::Code("0000".into()),
            },
        }),
        encode(Response { id: 8, outcome: Outcome::Error("bad request".into()) }),
    ]
}

/// One request line, read as one of novad's own commands.
fn decode_request(line: &str) -> Result<Request<Command>, String> {
    nova_carrier::decode_request(line)
}

#[test]
fn colors_are_six_digit_hex() {
    assert_eq!("#fd7014".parse::<Color>().unwrap().rgb(), [0xFD, 0x70, 0x14]);
    assert_eq!(String::from("#fd7014".parse::<Color>().unwrap()), "#FD7014");
    for bad in ["fd7014", "#fd701", "#fd70144", "#fd70zz", "#ffffffff", "#ééé"] {
        assert!(bad.parse::<Color>().is_err(), "{bad} parsed");
    }
}

#[test]
fn subscribe_parses_with_topics() {
    let req = decode_request(r#"{"v":2,"id":1,"cmd":"subscribe","args":{"topics":["features","logs"]}}"#).unwrap();
    assert_eq!(req.command, Command::Subscribe { topics: vec!["features".into(), "logs".into()] });
    // novad refuses a topic it doesn't have (its `requests` tests).
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
    let features = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../features");
    for panel in Panel::ALL {
        assert!(features.join(panel.name()).join("feature.toml").is_file(), "{panel} has no feature");
    }
}

/// The framework's log feed and novad's `logs` event and topic are one
/// thing on the wire.
#[test]
fn the_log_feed_is_the_logs_event() {
    let logs = Logs { backlog: true, entries: vec![log_entry()] };
    assert_eq!(encode(logs.event()).unwrap(), encode(Event::Logs(logs)).unwrap());
}

/// The adapters' compositor event, published without a copy, and
/// `nova_desktop_info::Event` are one thing on the wire.
#[test]
fn the_desktop_is_the_compositor_event() {
    let desktop = desktop();
    let event = nova_desktop_info::Event::Compositor(desktop.clone());
    assert_eq!(encode(nova_compositor::event(&desktop)).unwrap(), encode(event).unwrap());
}

#[test]
fn only_hello_has_no_topic() {
    assert_eq!(Event::Hello { daemon: "x".into() }.topic(), None);
    assert_eq!(Event::Features { features: vec![] }.topic(), Some(topics::FEATURES));
    assert_eq!(Event::Logs(Logs { backlog: false, entries: vec![] }).topic(), Some(topics::LOGS));
    assert_eq!(nova_theme_info::Event::Theme(theme()).topic(), Some(nova_theme_info::TOPIC));
    assert_eq!(nova_desktop_info::Event::Compositor(desktop()).topic(), Some(nova_desktop_info::TOPIC));
    let toggle = nova_bar_api::Event::Toggle { panel: Panel::AiUsage, output: None };
    assert_eq!(toggle.topic(), Some(nova_bar_api::TOPIC));
    assert_eq!(nova_volume_osd_api::Event::Volume { key: VolumeKey::Up }.topic(), Some(nova_volume_osd_api::TOPIC));
    assert_eq!(nova_host_info::Event::Host(host()).topic(), Some(nova_host_info::TOPIC));
}

#[test]
fn unknown_argument_fields_are_rejected() {
    let error = decode_request(r#"{"v":2,"cmd":"subscribe","args":{"topics":["logs"],"unexpected":true}}"#);
    assert!(error.unwrap_err().contains("unknown field `unexpected`"));
    assert!(decode_request(r#"{"v":2,"cmd":"ping","args":{"unexpected":true}}"#).is_err());
    let limit = r#"{"window":"weekly","percent":5,"extra":1}"#;
    let line = format!(r#"{{"v":2,"cmd":"report_claude_usage","args":{{"limits":[{limit}]}}}}"#);
    assert!(nova_carrier::decode_request::<nova_ai_usage_api::Command>(&line).is_err());
    let rect = r#"{"x":0,"y":0,"width":1,"height":1,"scale":2}"#;
    let line = format!(r#"{{"v":2,"cmd":"save_capture","args":{{"output":"DP-1","rect":{rect}}}}}"#);
    assert!(nova_carrier::decode_request::<nova_screenshot_api::Command>(&line).is_err());
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
fn host_rejects_unknown_fields() {
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
