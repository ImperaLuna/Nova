pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../stage"
import "service"

// The quick settings drawer, in the stage.panels host (view
// `quick-settings.panel`), while its service is ready. It grows out of
// the bar's right pill (view `right-pill`), when there is one.
Scope {
    id: slot

    // The screen's stage (Stage.qml).
    required property var stage
    readonly property list<StagePanel> panels: loader.item ? [loader.item] : []
    // The right pill on the bar, or null.
    readonly property Item rightPill: stage.barView("right-pill")?.pill ?? null

    LazyLoader {
        id: loader
        active: QuickSettingsService.ready
        component: QuickSettings {
            output: slot.stage.output
            stage: slot.stage
            pillItem: slot.rightPill
            // Where the right pill is, in screen coordinates (the stage's
            // top-left is the screen's), as it slides with the bar.
            Binding on pill {
                when: slot.rightPill !== null
                // Evaluated even while `when` is false.
                value: slot.rightPill ? Qt.rect(slot.rightPill.x, slot.stage.barOffset + slot.rightPill.y, slot.rightPill.width, slot.rightPill.height) : Qt.rect(0, 0, 0, 0)
            }
        }
    }
}
