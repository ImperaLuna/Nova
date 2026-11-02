import QtQuick
import "../theme"
import "capture.js" as Capture
import "../text/duration.js" as Duration

// The recording pill, left of the right pill while a recording runs: a red
// dot and how long it has recorded. The pointer on it shows pause (when the
// recorder can) and stop; a click anywhere else on it stops too. Glass,
// like the other pills, and a fixed shape on the stage.
Item {
    id: pill

    // How long it has recorded, in ms.
    property real elapsed: 0
    property bool paused: false
    property bool canPause: false

    signal pauseRequested(bool paused)
    signal stopRequested

    readonly property bool glass: true
    readonly property real radius: height / 2
    readonly property bool open: hover.hovered

    implicitWidth: row.implicitWidth + 2 * Tokens.pad
    implicitHeight: Tokens.barHeight

    HoverHandler {
        id: hover
    }

    GlassShadow {
        glass: surface
    }

    Glass {
        id: surface
        anchors.fill: parent
        radius: pill.radius
    }

    // The rest of the pill stops it; the buttons are on top.
    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: pill.stopRequested()
    }

    Row {
        id: row
        anchors.centerIn: parent
        spacing: Tokens.iconGap

        Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            width: Tokens.recordDotSize
            height: width
            radius: width / 2
            color: pill.paused ? Tokens.fgDim : Tokens.error
        }

        Text {
            anchors.verticalCenter: parent.verticalCenter
            text: Duration.clock(pill.elapsed)
            textFormat: Text.PlainText
            color: pill.paused ? Tokens.fgDim : Tokens.fg
            font.family: Tokens.fontMono
            font.pixelSize: Tokens.fontSize
        }

        PillIconButton {
            visible: pill.open && pill.canPause
            name: pill.paused ? Capture.ICON_RESUME : Capture.ICON_PAUSE
            onClicked: pill.pauseRequested(!pill.paused)
        }
        PillIconButton {
            visible: pill.open
            name: Capture.ICON_STOP
            ink: Tokens.error
            onClicked: pill.stopRequested()
        }
    }

    component PillIconButton: Rectangle {
        id: button
        required property string name
        property color ink: Tokens.fg
        signal clicked

        anchors.verticalCenter: parent.verticalCenter
        width: Tokens.pillButtonSize
        height: Tokens.pillButtonSize
        radius: width / 2
        color: press.containsMouse ? Tokens.hover : "transparent"

        CaptureIcon {
            anchors.centerIn: parent
            name: button.name
            color: button.ink
        }

        MouseArea {
            id: press
            anchors.fill: parent
            hoverEnabled: true
            onClicked: button.clicked()
        }
    }
}
