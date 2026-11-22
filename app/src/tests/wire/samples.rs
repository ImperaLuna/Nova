//! The sample values the wire snapshot shows: one of each message's
//! payloads, filled the way novad fills them.

use super::*;

pub(super) fn desktop() -> Desktop {
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

pub(super) fn ai_usage() -> nova_ai_usage_api::AiUsage {
    use nova_ai_usage_api::{AiProvider, AiUsage, ProviderUsage, UsageLimit, UsageState, UsageWindow};
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

pub(super) fn host() -> Host {
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

pub(super) fn resources() -> nova_resources_api::Resources {
    use nova_resources_api::{AppUsage, ResourceSample, Resources};
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

pub(super) fn launcher() -> nova_launcher_api::Launcher {
    nova_launcher_api::Launcher {
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

pub(super) fn network() -> nova_network_api::Network {
    use nova_network_api::{
        AttemptOutcome, LinkKind, LinkState, Network, Wifi, WifiAttempt, WifiBand, WifiNetwork, WifiSecurity, WiredLink,
    };
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

pub(super) fn bluetooth() -> nova_bluetooth_api::Bluetooth {
    use nova_bluetooth_api::{
        Bluetooth, BluetoothDevice, BluetoothKind, BluetoothRadio, BluetoothStatus, PairingAsk, PairingRequest,
    };
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

pub(super) fn media() -> nova_media_api::Media {
    use nova_media_api::{Media, MediaPlayer, PlaybackStatus};
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

pub(super) fn tray() -> nova_tray_api::Tray {
    use nova_tray_api::{Tray, TrayItem};
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

pub(super) fn peripherals() -> nova_peripherals_api::Peripherals {
    use nova_peripherals_api::{
        Battery, BatteryLevel, Peripheral, PeripheralKind, PeripheralLink, PeripheralSource, Peripherals,
    };
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

pub(super) fn notifications() -> nova_notifications_api::NotificationList {
    use nova_notifications_api::{
        Notification, NotificationAction, NotificationApp, NotificationDevice, NotificationList, Urgency,
    };
    use nova_peripherals_api::{Battery, BatteryLevel, PeripheralKind};
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

pub(super) fn log_entry() -> LogEntry {
    LogEntry {
        time: "2026-10-22T13:45:09.123Z".into(),
        level: LogLevel::Warn,
        source: UI_LOG_SOURCE.into(),
        message: "Binding loop detected: x: y".into(),
    }
}

pub(super) fn theme() -> Theme {
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

pub(super) fn status() -> Status {
    Status {
        daemon: "0.1.0".into(),
        pid: 100,
        uptime_secs: 42,
        pss_kib: Some(4096),
        quickshell: Some("0.3.2".into()),
        ui: UiStatus {
            program: UiProgram::Quickshell,
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
