pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../stage"
import "service"

// The power menu on the right edge, in the stage.panels host (view
// `power.menu`), while its service is ready.
Scope {
    id: slot

    // The screen's stage (Stage.qml).
    required property var stage
    readonly property list<StagePanel> panels: loader.item ? [loader.item] : []

    LazyLoader {
        id: loader
        active: PowerService.ready
        component: PowerMenu {
            output: slot.stage.output
        }
    }
}
