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
import "../media" as Media
import "../power" as Power
import "../quick_settings" as QuickSettings
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
// host: ui/bar/BarRow.qml, ui/bar/PillButtons.qml, ui/stage/PanelSlots.qml).
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
            "wallpaper.carousel": "../wallpaper/WallpaperSlot.qml"
        })

    // The URL of view `view`'s file, or "" for a view there isn't.
    function url(view: string): url {
        return files.hasOwnProperty(view) ? Qt.resolvedUrl(files[view]) : "";
    }
}
