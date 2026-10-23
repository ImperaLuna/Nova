import QtQuick
import Quickshell
import "../theme"
import "../daemon"
import "../ai_usage"
import "../ai_usage/service"

// Placeholder bar: one floating pill per screen, plus the AI usage pill on
// the left when that feature is on. The real bar (three pills, hide
// policies, reveal strip) lands with M2.
//
// It's drawn on the screen's stage (ui/stage/Stage.qml), which builds its
// input mask and blur from `shapes` and `glassShapes`, so panels can grow out
// of the pills without a window of their own.
Item {
    id: bar

    // The space kept free for the bar at the top of the screen.
    readonly property int reservedHeight: Tokens.barHeight + Tokens.barMargin

    readonly property AiUsage aiUsagePill: aiUsage.item as AiUsage
    // What's drawn and takes clicks, each rounded by its `radius`.
    readonly property list<Item> shapes: aiUsagePill ? [center, aiUsagePill] : [center]
    // The glass among them, which the compositor blurs behind.
    readonly property list<Item> glassShapes: aiUsagePill ? [aiUsagePill] : []
    // Whether an open panel takes typing (the launcher will); while it does,
    // the stage holds the keyboard and Esc or a click outside calls dismiss().
    readonly property bool needsKeyboard: false

    // Closes whatever is open.
    function dismiss() {
        if (aiUsagePill)
            aiUsagePill.open = false;
    }

    // Logs once the bar has drawn a frame while connected to novad, so the
    // VM tests know when to screenshot it.
    property bool shownLogged: false

    Connections {
        target: label.Window.window
        enabled: !bar.shownLogged
        function onFrameSwapped() {
            if (!Daemon.connected)
                return;
            bar.shownLogged = true;
            console.info("nova: bar shown");
        }
    }

    // MEASUREMENT ONLY (IMP-31): opens and closes the panel every
    // NOVA_MEASURE_TOGGLE_MS, and logs frame intervals while it morphs.
    Timer {
        interval: Number(Quickshell.env("NOVA_MEASURE_TOGGLE_MS") || 0)
        running: interval > 0 && bar.aiUsagePill !== null
        repeat: true
        onTriggered: bar.aiUsagePill.open = !bar.aiUsagePill.open
    }
    property real lastSwap: 0
    property int tailFrames: 0
    Connections {
        target: label.Window.window
        enabled: Quickshell.env("NOVA_MEASURE_TOGGLE_MS") !== null
        function onFrameSwapped() {
            const now = Date.now();
            const pill = bar.aiUsagePill;
            if (pill && pill.t > 0 && pill.t < 1)
                bar.tailFrames = 3;
            if (bar.tailFrames > 0) {
                bar.tailFrames--;
                console.info(`nova-measure: frame dt=${now - bar.lastSwap} t=${pill.t.toFixed(3)} open=${pill.open} screen=${label.Window.window.screen ? label.Window.window.screen.name : "?"}`);
            }
            bar.lastSwap = now;
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
            measureShadow: Quickshell.env("NOVA_MEASURE_SHADOW") || "cut"
            measureUnload: Quickshell.env("NOVA_MEASURE_UNLOAD") === "1"
            onShowRequested: provider => AiUsageService.show(provider)
            onRefreshRequested: AiUsageService.refresh()
        }
    }

    Rectangle {
        id: center
        anchors.horizontalCenter: parent.horizontalCenter
        y: Tokens.barMargin
        height: Tokens.barHeight
        width: label.implicitWidth + Tokens.pad * 2
        radius: Tokens.radiusPill
        color: Tokens.surface
        border.color: Tokens.outline

        Text {
            id: label
            anchors.centerIn: parent
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            color: Daemon.connected ? Tokens.fg : Tokens.fgDim
            textFormat: Text.PlainText
            text: Daemon.connected ? `Nova · novad ${Daemon.version}` : "Nova · waiting for novad"
        }
    }
}
