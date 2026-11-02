import QtQuick
import Quickshell
import "../bar"
import "../compositor"
import "../daemon"
import "../daemon/protocol.js" as Protocol
import "../launcher/service"
import "../slots/slots.js" as Slots
import "../theme"

// The central pill in the bar.center row (view `central-pill`), and the
// central-pill.parts host: that screen's workspaces from the compositor
// (view `workspaces.list`), the time from a clock that ticks on the minute
// or the bar's frozen time (view `clock.time`), and the search button while
// the launcher is on. Its time sits in the middle of the screen; without
// the clock, the pill does. With none of them it's left out.
BarSlot {
    id: slot

    // The parts whose features are ready.
    readonly property var parts: Slots.forHost(Daemon.features, Slots.HOST_CENTRAL_PILL_PARTS).map(part => part.view)

    pill: centralPill
    middle: centralPill.showClock ? centralPill.width - centralPill.timeFromRight : centralPill.width / 2
    present: !centralPill.empty
    shapes: centralPill.empty ? [] : [centralPill]

    // Ticks once a minute, on the minute, for the time.
    SystemClock {
        id: clock
        enabled: !slot.bar.frozenTime && centralPill.showClock
        precision: SystemClock.Minutes
    }

    CentralPill {
        id: centralPill
        readonly property var output: Compositor.output(slot.bar.screenName)

        x: slot.slotX
        y: Tokens.barGap
        width: implicitWidth
        height: implicitHeight
        visible: !empty
        workspaces: slot.parts.includes(Slots.PART_WORKSPACES) && Compositor.has(Protocol.CAPABILITY_WORKSPACES) ? Protocol.barWorkspaces(output) : []
        showClock: slot.parts.includes(Slots.PART_CLOCK)
        outputFocused: output?.focused ?? false
        canFocus: Compositor.has(Protocol.CAPABILITY_FOCUS_WORKSPACE)
        canSearch: LauncherService.ready
        onSearchRequested: slot.stage.toggle(Protocol.FEATURE_LAUNCHER)
        now: slot.bar.frozenTime ? new Date(slot.bar.frozenTime) : clock.date
        onFocusRequested: workspaceId => Compositor.focusWorkspace(workspaceId)
        onStepRequested: step => Compositor.stepWorkspace(slot.bar.screenName, step)
    }
}
