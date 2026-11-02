.pragma library

// Every slot view: its id (a manifest's `[[slot]] view`) and the QML file
// that draws it, relative to ui/. Manifests name ids, never files, so they
// stay the same whatever draws them. A Rust test (nova-features) checks
// that every view a manifest names is here and that its file exists.
//
// Each file is a thin adapter: it reads its feature's service and passes
// props to a view that only draws them, so ui/tests can test the view with
// recorded data. Its root takes what its host gives every view (see the
// host: ui/bar/BarRow.qml, ui/bar/PillButtons.qml, ui/stage/PanelSlots.qml).
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
    "wallpaper.carousel": "wallpaper/WallpaperSlot.qml"
};

// The file of view `view`, relative to ui/, or "" for one there isn't.
function path(view) {
    return VIEWS.hasOwnProperty(view) ? VIEWS[view] : "";
}
