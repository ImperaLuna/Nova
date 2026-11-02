pragma Singleton

import QtQuick
// Every folder a view is in. Quickshell only loads the QML files of
// folders some file imports, and a view is loaded by its URL, so this is
// what makes each view's folder (and the types the view uses there)
// available. A Rust test (nova-features) checks every view's folder is
// here. Nothing here uses them by name, hence the qmllint comment.
// qmllint disable unused-imports
import "../ai_usage" as AiUsage
import "../bar" as Bar
import "../capture" as Capture
import "../central_pill" as CentralPill
import "../launcher" as Launcher
import "../lock" as Lock
import "../media" as Media
import "../notifications" as Notifications
import "../power" as Power
import "../quick_settings" as QuickSettings
import "../quick_settings/slots" as QuickSettingsSlots
import "../volume" as Volume
import "../wallpaper" as Wallpaper

// qmllint enable unused-imports

// Every slot view: its id (a manifest's `[[slot]] view`) and the QML file
// that draws it, relative to this folder. Manifests name ids, never files,
// so they stay the same whatever draws them. A Rust test (nova-features)
// checks that every view a manifest names is here and that its file
// exists.
//
// Each file is a thin adapter: it reads its feature's service and passes
// props to a view that only draws them, so ui/tests can test the view with
// recorded data. Its root takes what its host gives every view (see the
// host: ui/bar/BarRow.qml, ui/bar/PillButtons.qml, ui/stage/PanelSlots.qml,
// ui/quick_settings/MainPage.qml and QuickSettings.qml).
QtObject {
    readonly property var files: ({
            "ai-usage.pill": "../ai_usage/AiUsageSlot.qml",
            "media.pill": "../media/NowPlayingSlot.qml",
            "central-pill": "../central_pill/CentralPillSlot.qml",
            "recording.pill": "../capture/RecordingPillSlot.qml",
            "right-pill": "../bar/RightPillSlot.qml",
            "quick-settings.button": "../quick_settings/QuickSettingsButtonSlot.qml",
            "power.button": "../power/PowerButtonSlot.qml",
            "power.menu": "../power/PowerMenuSlot.qml",
            "quick-settings.panel": "../quick_settings/QuickSettingsSlot.qml",
            "launcher.panel": "../launcher/LauncherSlot.qml",
            "wallpaper.carousel": "../wallpaper/WallpaperSlot.qml",
            "network.tile": "../quick_settings/slots/NetworkTileSlot.qml",
            "bluetooth.tile": "../quick_settings/slots/BluetoothTileSlot.qml",
            "keep-awake.tile": "../quick_settings/slots/KeepAwakeTileSlot.qml",
            "microphone.tile": "../quick_settings/slots/MicrophoneTileSlot.qml",
            "sound.slider": "../quick_settings/slots/SoundSliderSlot.qml",
            "microphone.slider": "../quick_settings/slots/MicrophoneSliderSlot.qml",
            "peripherals.card": "../quick_settings/slots/DevicesCardSlot.qml",
            "tray.card": "../quick_settings/slots/TrayCardSlot.qml",
            "notifications.list": "../quick_settings/slots/NotificationListSlot.qml",
            "resources.card": "../quick_settings/slots/ResourcesCardSlot.qml",
            "sound.page": "../quick_settings/slots/SoundPageSlot.qml",
            "network.page": "../quick_settings/slots/NetworkPageSlot.qml",
            "bluetooth.page": "../quick_settings/slots/BluetoothPageSlot.qml",
            "resources.page": "../quick_settings/slots/ResourcesPageSlot.qml",
            "notifications.page": "../quick_settings/slots/NotificationSettingsPageSlot.qml",
            // The central pill draws its parts itself, the ones whose slots
            // are filled (central-pill.parts).
            "workspaces.list": "../central_pill/CentralPill.qml",
            "clock.time": "../central_pill/CentralPill.qml",
            "volume-osd.capsule": "../volume/VolumeOsd.qml",
            "lock.screen": "../lock/LockScreenSlot.qml",
            "notifications.popups": "../notifications/NotificationPopups.qml",
            "bluetooth.pairing": "../quick_settings/PairingPopup.qml",
            "capture.overlay": "../capture/CaptureSlot.qml"
        })

    // The URL of view `view`'s file, or "" for a view there isn't.
    function url(view: string): url {
        return files.hasOwnProperty(view) ? Qt.resolvedUrl(files[view]) : "";
    }
}
