import QtQuick
import Quickshell
import "../theme"
import "../daemon"

// Placeholder bar: one floating pill per screen. The real bar (three pills,
// hide policies, reveal strip) lands with M2.
PanelWindow { // qmllint disable uncreatable-type
    id: bar

    anchors {
        top: true
        left: true
        right: true
    }
    implicitHeight: Tokens.barHeight + Tokens.barMargin
    color: "transparent"

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

    Rectangle {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
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
