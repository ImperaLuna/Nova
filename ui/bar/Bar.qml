import QtQuick
import Quickshell
import "../theme"
import "../daemon"
import "../daemon/protocol.js" as Protocol
import "../compositor"
import "../ai_usage"
import "../ai_usage/service"
import "../central_pill"
import "../launcher/service"
import "../stage"

// The bar on one screen: the AI usage pill on the left when that feature is
// on, and the central pill (the launcher's search button, that screen's
// workspaces, the day and the time) with its time in the middle of the
// screen. The right pill and the hide
// policies come later.
//
// It's drawn on the screen's stage (ui/stage/Stage.qml), which builds its
// input mask, blur and keyboard grab from `panels` and `fixedShapes`, so
// panels can grow out of the pills without a window of their own. A new
// pill that opens a panel is a StagePanel added to `panels`.
Item {
    id: bar

    // The screen's output name (Quickshell's `screen.name`), whose
    // workspaces the central pill lists.
    required property string screenName

    // The space kept free for the bar at the top of the screen.
    readonly property int reservedHeight: Tokens.barHeight + Tokens.barMargin

    readonly property AiUsage aiUsagePill: aiUsage.item as AiUsage
    // The pills that open panels, for the stage (StagePanel has the contract).
    readonly property list<StagePanel> panels: aiUsagePill ? [aiUsagePill] : []
    // What else is drawn and takes clicks, each rounded by its `radius`.
    readonly property list<Item> fixedShapes: [center]
    // The pill in the middle, which a panel in the middle of the screen can
    // grow out of.
    readonly property Item centralPill: center
    // Its items a panel on another surface is drawn over (StagePanel
    // `from`): hidden meanwhile. The stage sets it.
    property list<Item> covered: []

    // The central pill's search button was clicked: open or close the
    // launcher on this screen.
    signal launcherRequested

    // Logs once the bar has drawn a frame while connected to novad, so the
    // VM tests know when to screenshot it.
    property bool shownLogged: false

    Connections {
        target: center.Window.window
        enabled: !bar.shownLogged
        function onFrameSwapped() {
            if (!Daemon.connected)
                return;
            bar.shownLogged = true;
            console.info("nova: bar shown");
        }
    }

    Loader {
        id: aiUsage
        x: Tokens.barMargin
        y: Tokens.barMargin
        active: AiUsageService.ready
        onLoaded: console.info("nova: ai usage pill shown")
        sourceComponent: AiUsage {
            usage: AiUsageService.usage
            onShowRequested: provider => AiUsageService.show(provider)
            onRefreshRequested: AiUsageService.refresh()
        }
    }

    // Ticks once a minute, on the minute, for the central pill's time.
    SystemClock {
        id: clock
        enabled: !bar.frozenTime
        precision: SystemClock.Minutes
    }
    // A fixed time to show instead (an ISO date, like 2026-10-24T16:40), so
    // the VM tests' screenshots don't change with the clock.
    readonly property string frozenTime: Quickshell.env("NOVA_FROZEN_TIME") ?? ""

    CentralPill {
        id: center
        readonly property var output: Compositor.output(bar.screenName)

        visible: !bar.covered.includes(center)
        x: bar.width / 2 + timeFromRight - width
        y: Tokens.barMargin
        width: implicitWidth
        height: implicitHeight
        workspaces: Compositor.has(Protocol.CAPABILITY_WORKSPACES) ? Protocol.barWorkspaces(output) : []
        outputFocused: output?.focused ?? false
        canFocus: Compositor.has(Protocol.CAPABILITY_FOCUS_WORKSPACE)
        canSearch: LauncherService.ready
        onSearchRequested: bar.launcherRequested()
        now: bar.frozenTime ? new Date(bar.frozenTime) : clock.date
        onFocusRequested: workspaceId => Compositor.focusWorkspace(workspaceId)
        onStepRequested: step => Compositor.stepWorkspace(bar.screenName, step)
    }
}
