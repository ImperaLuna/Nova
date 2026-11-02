.pragma library

// Every slot view: its id (a manifest's `[[slot]] view`) and the QML file
// that draws it, relative to ui/. Manifests name ids, never files, so they
// stay the same whatever draws them. A Rust test (nova-features) checks
// that every view a manifest names is here and that its file exists.
//
// Each file is a thin adapter: it reads its feature's service and passes
// props to a view that only draws them, so ui/tests can test the view with
// recorded data. Its root takes what its host gives every view (see the
// host: ui/bar/BarRow.qml, ui/bar/PillButtons.qml, ui/stage/PanelSlots.qml,
// ui/quick_settings/MainPage.qml and QuickSettings.qml).
var VIEWS = {
    "ai-usage.pill": "ai_usage/AiUsageSlot.qml",
    "media.pill": "media/NowPlayingSlot.qml",
    "central-pill": "central_pill/CentralPillSlot.qml",
    "recording.pill": "capture/RecordingPillSlot.qml",
    "right-pill": "bar/RightPillSlot.qml",
    "quick-settings.button": "quick_settings/QuickSettingsButtonSlot.qml",
    "power.button": "power/PowerButtonSlot.qml",
    "power.menu": "power/PowerMenuSlot.qml",
    "quick-settings.panel": "quick_settings/QuickSettingsSlot.qml",
    "launcher.panel": "launcher/LauncherSlot.qml",
    "wallpaper.carousel": "wallpaper/WallpaperSlot.qml",
    "network.tile": "quick_settings/slots/NetworkTileSlot.qml",
    "bluetooth.tile": "quick_settings/slots/BluetoothTileSlot.qml",
    "keep-awake.tile": "quick_settings/slots/KeepAwakeTileSlot.qml",
    "microphone.tile": "quick_settings/slots/MicrophoneTileSlot.qml",
    "sound.slider": "quick_settings/slots/SoundSliderSlot.qml",
    "microphone.slider": "quick_settings/slots/MicrophoneSliderSlot.qml",
    "peripherals.card": "quick_settings/slots/DevicesCardSlot.qml",
    "tray.card": "quick_settings/slots/TrayCardSlot.qml",
    "notifications.list": "quick_settings/slots/NotificationListSlot.qml",
    "resources.card": "quick_settings/slots/ResourcesCardSlot.qml",
    "sound.page": "quick_settings/slots/SoundPageSlot.qml",
    "network.page": "quick_settings/slots/NetworkPageSlot.qml",
    "bluetooth.page": "quick_settings/slots/BluetoothPageSlot.qml",
    "resources.page": "quick_settings/slots/ResourcesPageSlot.qml",
    "notifications.page": "quick_settings/slots/NotificationSettingsPageSlot.qml",
    // The central pill draws its parts itself, the ones whose slots are
    // filled (central-pill.parts).
    "workspaces.list": "central_pill/CentralPill.qml",
    "clock.time": "central_pill/CentralPill.qml",
    "volume-osd.capsule": "volume/VolumeOsd.qml",
    "lock.screen": "lock/LockScreenSlot.qml",
    "notifications.popups": "notifications/NotificationPopups.qml",
    "bluetooth.pairing": "quick_settings/PairingPopup.qml",
    "capture.overlay": "capture/CaptureSlot.qml"
};

// The file of view `view`, relative to ui/, or "" for one there isn't.
function path(view) {
    return VIEWS.hasOwnProperty(view) ? VIEWS[view] : "";
}
