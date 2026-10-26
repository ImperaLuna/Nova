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
import "../capture/service"
import "../stage"
import "../tray"
import "../tray/service"

// The bar on one screen: the AI usage pill on the left when that feature is
// on, the central pill (the launcher's search button, that screen's
// workspaces, the day and the time) with its time in the middle of the
// screen, and the right pill with the quick settings and power buttons;
// while a screen recording runs, its pill left of the right one.
// The hide policies come later.
//
// While the screen shows a workspace with a window over the whole screen
// (fullscreen), the bar steps aside: the pills slide up out of sight, then
// its space is given back, so the windows beside the fullscreen one get the
// full height. When that ends the space comes back first, then the pills.
// Meanwhile touching the top edge brings the pills back over the windows,
// and they stay while one of their panels is open (`held`).
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
    // The monitor's height: the bar itself is only as tall as the stage.
    property real screenHeight: 0

    // The space kept free for the bar at the top of the screen: the same gap
    // above and below the pills, none while it steps aside for a fullscreen
    // window. The compositor's own top gap is meant to be off, so it doesn't
    // add to the one below.
    readonly property int reservedHeight: spaceKept ? Tokens.barGap + Tokens.barHeight + Tokens.barGap : 0

    // The shown workspace has a window over the whole screen.
    readonly property bool fullscreenBelow: Compositor.has(Protocol.CAPABILITY_FULLSCREEN) && Protocol.fullscreenShown(Compositor.output(screenName))
    // One of the bar's panels is open, so the pills stay. The stage sets it.
    property bool held: false
    // Brought back over a fullscreen window by the pointer at the top edge.
    readonly property bool peeking: fullscreenBelow && (pointer.hovered || linger.running)
    // Slid up out of sight.
    readonly property bool tucked: (fullscreenBelow || waitingForSpace) && !peeking && !held
    // Sliding in or out, so the stage draws.
    readonly property bool sliding: slide.running
    // How far the pills sit above their place: they slide with their shelf,
    // which moves them without changing their own position.
    readonly property real slideOffset: shelf.y
    property bool spaceKept: true
    // Tucked pills wait while the space comes back.
    property bool waitingForSpace: false

    onFullscreenBelowChanged: {
        if (fullscreenBelow) {
            spaceReturning.stop();
            waitingForSpace = false;
            spaceGoing.restart();
        } else {
            spaceGoing.stop();
            // Only pills that were out of sight wait; shown ones stay put.
            waitingForSpace = !held && !pointer.hovered && !linger.running;
            spaceKept = true;
            spaceReturning.restart();
        }
    }
    onSpaceKeptChanged: console.info(spaceKept ? "nova: bar space kept" : "nova: bar space given to a fullscreen window")

    // The space goes once the pills have slid away...
    Timer {
        id: spaceGoing
        interval: Tokens.durationPanel
        onTriggered: bar.spaceKept = false
    }
    // ...and comes back before they slide in, so they never land on a window.
    Timer {
        id: spaceReturning
        interval: Tokens.durationPanel
        onTriggered: bar.waitingForSpace = false
    }
    Timer {
        id: linger
        interval: Tokens.durationPeekLinger
    }

    // On the reveal strip or a pill: the stage only takes the pointer there.
    HoverHandler {
        id: pointer
        onHoveredChanged: {
            if (!hovered)
                linger.restart();
        }
    }

    readonly property AiUsage aiUsagePill: aiUsage.item as AiUsage
    // The pills that open panels, for the stage (StagePanel has the contract).
    readonly property list<StagePanel> panels: aiUsagePill ? [aiUsagePill, rightPill, trayMenu] : [rightPill, trayMenu]
    // What else is drawn and takes clicks, each rounded by its `radius`.
    readonly property list<Item> fixedShapes: [center, recordingPill, revealStrip]
    // The pill in the middle, which a panel in the middle of the screen can
    // grow out of.
    readonly property Item centralPill: center
    // Its items a panel on another surface is drawn over (StagePanel
    // `from`): hidden meanwhile. The stage sets it.
    property list<Item> covered: []

    // The right pill's buttons: whether their features are on, and whether
    // quick settings is open on this screen.
    property bool quickSettings: false
    property bool power: false
    property bool quickSettingsOpen: false
    // Unread notifications and do not disturb, for the quick settings
    // button's badge (PillButtons).
    property int unread: 0
    property bool doNotDisturb: false
    signal quickSettingsClicked
    signal powerClicked
    // Where the right pill is, in screen coordinates (the stage's top-left
    // is the screen's): quick settings grows out of it.
    readonly property rect rightPillBox: Qt.rect(rightPill.x, shelf.y + rightPill.y, rightPill.width, rightPill.height)
    readonly property Item rightPillItem: rightPill
    // Opens the menu of the tray app `entry`, whose icon's middle is `at`
    // in the right pill.
    function showTrayMenu(entry: var, at: point) {
        trayMenu.entry = entry;
        trayMenu.anchorX = rightPill.x + at.x;
        trayMenu.open = true;
    }

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

    // The pills, which slide up out of sight while the bar steps aside.
    Item {
        id: shelf
        width: bar.width
        height: bar.height
        y: bar.tucked ? -(Tokens.barGap + Tokens.barHeight + Tokens.shadowRoom) : 0
        opacity: bar.tucked ? 0 : 1
        // Gone once out of sight, so they take no clicks and need no blur.
        visible: !bar.tucked || slide.running

        Behavior on y {
            NumberAnimation {
                id: slide
                duration: Tokens.durationPanel
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }
        Behavior on opacity {
            NumberAnimation {
                duration: Tokens.durationPanel
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }

        Loader {
            id: aiUsage
            x: Tokens.barMargin
            y: Tokens.barGap
            active: AiUsageService.ready
            onLoaded: console.info("nova: ai usage pill shown")
            sourceComponent: AiUsage {
                usage: AiUsageService.usage
                onShowRequested: provider => AiUsageService.show(provider)
                onRefreshRequested: AiUsageService.refresh()
            }
        }

        CentralPill {
            id: center
            readonly property var output: Compositor.output(bar.screenName)

            visible: !bar.covered.includes(center)
            x: bar.width / 2 + timeFromRight - width
            y: Tokens.barGap
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

        RightPill {
            id: rightPill
            // Also for the tray alone, with quick settings and power off.
            visible: (bar.quickSettings || bar.power || rightPill.hasTray) && !bar.covered.includes(rightPill)
            x: bar.width - Tokens.barMargin - width
            y: Tokens.barGap
            width: implicitWidth
            height: implicitHeight
            quickSettings: bar.quickSettings
            power: bar.power
            quickSettingsOpen: bar.quickSettingsOpen
            unread: bar.unread
            doNotDisturb: bar.doNotDisturb
            onQuickSettingsClicked: bar.quickSettingsClicked()
            onPowerClicked: bar.powerClicked()
            trayApps: TrayService.shown
            trayAttention: TrayService.attention
            iconSource: entry => TrayService.iconSource(entry)
            // A click acts and closes the tray; an app that only has a menu
            // shows it instead.
            onTrayActivated: (entry, at) => {
                if (TrayService.opensMenu(entry)) {
                    bar.showTrayMenu(entry, at);
                } else {
                    TrayService.activate(entry);
                    rightPill.open = false;
                }
            }
            onTrayMenuRequested: (entry, at) => bar.showTrayMenu(entry, at)
            onTraySecondaryActivated: entry => entry.item.secondaryActivate()
            onOpenChanged: {
                if (!open)
                    trayMenu.open = false;
            }
        }

        TrayMenu {
            id: trayMenu
            screenWidth: bar.width
            screenHeight: bar.screenHeight
            iconSource: entry ? TrayService.iconSource(entry) : ""
            onDone: rightPill.open = false
            onHideRequested: (key, hidden) => TrayService.hide(key, hidden)
        }

        RecordingPill {
            id: recordingPill
            visible: RecordingService.recording
            x: (rightPill.visible ? rightPill.x - Tokens.gap : bar.width - Tokens.barMargin) - width
            y: Tokens.barGap
            width: implicitWidth
            height: implicitHeight
            elapsed: RecordingService.elapsed
            paused: RecordingService.active?.paused ?? false
            canPause: RecordingService.active?.can_pause ?? false
            onPauseRequested: paused => RecordingService.pause(paused)
            onStopRequested: RecordingService.stop()
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

    // Along the top edge while the bar steps aside: the pointer touching it
    // brings the pills back. Nothing is drawn.
    Item {
        id: revealStrip
        readonly property int radius: 0
        visible: bar.fullscreenBelow
        width: bar.width
        height: Tokens.revealStrip
    }
}
