pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../icons"
import "../stage"
import "service"

// The launcher in the middle of the screen, in the stage.panels host
// (view `launcher.panel`), while its service is ready.
Scope {
    id: slot

    // The screen's stage (Stage.qml).
    required property var stage
    readonly property list<StagePanel> panels: loader.item ? [loader.item] : []

    LazyLoader {
        id: loader
        active: LauncherService.ready
        component: Launcher {
            id: deck
            screenWidth: slot.stage.output.width
            screenHeight: slot.stage.output.height
            apps: LauncherService.current.apps
            pinned: LauncherService.current.pinned
            recent: LauncherService.current.recent
            iconSource: icon => AppIcons.source(icon)
            onSearchRequested: query => LauncherService.search(query, found => deck.showMatches(query, found))
            onLaunchRequested: (id, newWindow) => LauncherService.launch(id, slot.stage.output.name, newWindow, error => deck.launched(error))
            onPinRequested: (id, pinned) => LauncherService.pin(id, pinned)
        }
    }
}
