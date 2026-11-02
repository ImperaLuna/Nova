import QtQuick
import Quickshell
import "../bar"
import "../compositor"
import "../daemon/protocol.js" as Protocol
import "../launcher/service"
import "../theme"

// The central pill in the bar.center row (view `central-pill`): that
// screen's workspaces from the compositor, the time from a clock that
// ticks on the minute (or the bar's frozen time), and the search button
// while the launcher is on. Its time sits in the middle of the screen.
BarSlot {
    id: slot

    pill: centralPill
    middle: centralPill.width - centralPill.timeFromRight
    shapes: [centralPill]

    // Ticks once a minute, on the minute, for the time.
    SystemClock {
        id: clock
        enabled: !slot.bar.frozenTime
        precision: SystemClock.Minutes
    }

    CentralPill {
        id: centralPill
        readonly property var output: Compositor.output(slot.bar.screenName)

        x: slot.slotX
        y: Tokens.barGap
        width: implicitWidth
        height: implicitHeight
        workspaces: Compositor.has(Protocol.CAPABILITY_WORKSPACES) ? Protocol.barWorkspaces(output) : []
        outputFocused: output?.focused ?? false
        canFocus: Compositor.has(Protocol.CAPABILITY_FOCUS_WORKSPACE)
        canSearch: LauncherService.ready
        onSearchRequested: slot.stage.toggle(Protocol.FEATURE_LAUNCHER)
        now: slot.bar.frozenTime ? new Date(slot.bar.frozenTime) : clock.date
        onFocusRequested: workspaceId => Compositor.focusWorkspace(workspaceId)
        onStepRequested: step => Compositor.stepWorkspace(slot.bar.screenName, step)
    }
}
