pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../theme"
import "../daemon"
import "../daemon/protocol.js" as Protocol
import "../compositor"
import "../stage"
import "../slots/slots.js" as Slots

// The bar on one screen: three rows of pills (BarRow), filled from the
// features' manifests (`[[slot]]` in the bar.left, bar.center and bar.right
// hosts) in their `order`. Today that's the AI usage pill and, right of it,
// the now-playing pill while something plays on the left; the central pill
// (the launcher's search button, that screen's workspaces, the day and the
// time) with its time in the middle of the screen; and the right pill with
// the tray and the right-pill.buttons host on the right, the recording
// pill left of it while a recording runs. A new pill needs only its own
// manifest slot and its folder (ui/slots/Views.qml maps its view id).
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
// panels can grow out of the pills without a window of their own. Each
// row collects them from its views (BarSlot).
//
// With the bar feature off its rows are empty (every bar.* host is the
// bar's), and it keeps no space and has no reveal strip: windows get the
// whole height, and panels still open from their keys and `nova toggle`.
Item {
    id: root

    // The screen's output name (Quickshell's `screen.name`), whose
    // workspaces the central pill lists.
    required property string screenName
    // The screen's stage (Stage.qml), which the pills open panels through.
    required property var stage
    // The monitor's height: the bar itself is only as tall as the stage.
    property real screenHeight: 0

    // The bar feature is on.
    readonly property bool shown: Daemon.readyFeatures.includes(Protocol.FEATURE_BAR)

    // The space kept free for the bar at the top of the screen: the same gap
    // above and below the pills, none while it steps aside for a fullscreen
    // window or with the bar off. The compositor's own top gap is meant to
    // be off, so it doesn't add to the one below.
    readonly property int reservedHeight: shown && spaceKept ? Tokens.barGap + Tokens.barHeight + Tokens.barGap : 0

    // The shown workspace has a window over the whole screen, so the bar
    // steps aside (it has none to step aside with the bar off).
    readonly property bool fullscreenBelow: shown && Compositor.has(Protocol.CAPABILITY_FULLSCREEN) && Protocol.fullscreenShown(Compositor.output(screenName))
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
        onTriggered: root.spaceKept = false
    }
    // ...and comes back before they slide in, so they never land on a window.
    Timer {
        id: spaceReturning
        interval: Tokens.durationPanel
        onTriggered: root.waitingForSpace = false
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

    readonly property list<BarRow> rows: [left, center, right]
    // The pills that open panels, for the stage (StagePanel has the contract).
    readonly property list<StagePanel> panels: rows.reduce((all, row) => [...all, ...row.panels], [])
    // What else is drawn and takes clicks, each rounded by its `radius`.
    readonly property list<Item> fixedShapes: [...rows.reduce((all, row) => [...all, ...row.shapes], []), revealStrip]
    // Its items a panel on another surface is drawn over (StagePanel
    // `from`): hidden meanwhile. The stage sets it.
    property list<Item> covered: []
    // Its items a panel of another pill is drawn over, like the now-playing
    // pill while AI usage is open over it (StagePanels `underPanels`): they
    // fade out meanwhile, then hide. The stage sets it.
    property list<Item> underPanels: []
    // One of them is fading, so the stage draws.
    readonly property bool fading: rows.some(row => row.fading)

    // The view with id `view` (a BarSlot) in any row, or null: a panel
    // elsewhere grows out of its pill.
    function view(id: string): BarSlot {
        return rows.map(row => row.view(id)).find(found => found !== null) ?? null;
    }

    // Logs once the bar has drawn a frame while connected to novad, so the
    // VM tests know when to screenshot it.
    property bool shownLogged: false

    Connections {
        target: root.Window.window
        enabled: !root.shownLogged
        function onFrameSwapped() {
            // Its pills come with the features event.
            if (!Daemon.connected || !root.rows.some(row => row.views.length > 0))
                return;
            root.shownLogged = true;
            console.info("nova: bar shown");
        }
    }

    // The pills, which slide up out of sight while the bar steps aside.
    Item {
        id: shelf
        width: root.width
        height: root.height
        y: root.tucked ? -(Tokens.barGap + Tokens.barHeight + Tokens.shadowRoom) : 0
        opacity: root.tucked ? 0 : 1
        // Gone once out of sight, so they take no clicks and need no blur.
        visible: !root.tucked || slide.running

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

        BarRow {
            id: left
            anchors.fill: parent
            host: Slots.HOST_BAR_LEFT
            shown: root.shown
            bar: root
            stage: root.stage
        }
        BarRow {
            id: center
            anchors.fill: parent
            host: Slots.HOST_BAR_CENTER
            shown: root.shown
            bar: root
            stage: root.stage
        }
        BarRow {
            id: right
            anchors.fill: parent
            host: Slots.HOST_BAR_RIGHT
            shown: root.shown
            bar: root
            stage: root.stage
        }
    }

    // A fixed time to show instead (an ISO date, like 2026-10-24T16:40), so
    // the VM tests' screenshots don't change with the clock.
    readonly property string frozenTime: Quickshell.env("NOVA_FROZEN_TIME") ?? ""
    // The now-playing equalizer stands still (NOVA_STILL_EQUALIZER=1), for
    // the same screenshots. Apart from the time, so a test can watch the
    // equalizer move while the clock stays put.
    readonly property bool stillEqualizer: Quickshell.env("NOVA_STILL_EQUALIZER") === "1"

    // Along the top edge while the bar steps aside: the pointer touching it
    // brings the pills back. Nothing is drawn.
    Item {
        id: revealStrip
        readonly property int radius: 0
        visible: root.fullscreenBelow
        width: root.width
        height: Tokens.revealStrip
    }
}
