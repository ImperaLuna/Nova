import Quickshell
import "../stage"

// The tray's stage.panels views (`tray.panel`, `tray.menu`) for nova-ui,
// which draws the open tray and an app's menu as panels of their own.
// Today's right pill draws both itself (ui/bar/RightPillSlot.qml), so here
// they have none.
Scope {
    // The screen's stage (Stage.qml).
    required property var stage
    readonly property list<StagePanel> panels: []
}
