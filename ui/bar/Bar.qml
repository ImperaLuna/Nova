import QtQuick
import "../theme"
import "../daemon"
import "../ai_usage"

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

    Loader {
        id: aiUsage
        x: Tokens.barMargin
        y: Tokens.barMargin
        active: Daemon.readyFeatures.includes("ai-usage") && Daemon.aiUsage !== null
        onLoaded: console.info("nova: ai usage pill shown")
        sourceComponent: AiUsage {
            usage: Daemon.aiUsage
            onShowRequested: provider => Daemon.call("show_ai_usage", {
                    provider: provider
                })
            onRefreshRequested: Daemon.call("refresh_ai_usage")
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
            text: Daemon.connected ? `Nova · novad ${Daemon.version}` : "Nova · waiting for novad"
        }
    }
}
