import QtQuick
import Quickshell
import Quickshell.Wayland
import "../theme"
import "../daemon"
import "../ai_usage"

// Placeholder bar: one floating pill per screen, plus the AI usage pill on
// the left when that feature is on. The real bar (three pills, hide
// policies, reveal strip) lands with M2.
//
// The window is taller than the bar when a pill has a panel to open, but it
// only reserves the bar's height and takes input only on the pills, so it
// never pushes windows around or blocks clicks between pills.
PanelWindow { // qmllint disable uncreatable-type
    id: bar

    anchors {
        top: true
        left: true
        right: true
    }
    exclusiveZone: Tokens.barHeight + Tokens.barMargin
    // Room below the open panel for its shadow; the mask keeps it click-through.
    implicitHeight: Tokens.barMargin + Math.max(Tokens.barHeight, ((aiUsage.item as AiUsage)?.openHeight ?? 0) + Tokens.shadowRoom)
    color: "transparent"

    mask: Region {
        Region {
            item: center
        }
        Region {
            item: aiUsage.item as Item
        }
    }

    // Real blur behind the glass where the compositor offers it
    // (ext-background-effect-v1, e.g. niri); elsewhere the tint stands alone.
    // The blur spike (IMP-9) decides what other compositors get.
    Region {
        id: blur
        Region {
            item: aiUsage.item as Item
            radius: (aiUsage.item as AiUsage)?.radius ?? 0
        }
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
        // Set here, not declared on the window: asking for blur at all logs a
        // warning on compositors without it, which a bar with no glass
        // shouldn't do.
        onLoaded: bar.BackgroundEffect.blurRegion = blur
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
