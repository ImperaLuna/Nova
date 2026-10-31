import QtQuick
import "../theme"

// One power menu action: its icon and label, filled with the accent while
// selected. Waiting for its second click it asks "Shut down?" with the
// seconds left and a bar draining under it; running, it shows a spinner.
// Dimmed with the reason under it when it can't run.
Item {
    id: row

    required property string action
    required property string label
    // "Shutting down" while it runs.
    required property string doing
    property bool selected: false
    property bool available: true
    property string reason: ""
    // Waiting for its second click: 1 down to 0 over Tokens.durationConfirm.
    property bool armed: false
    property real armedLeft: 0
    property bool running: false
    // Another row is running.
    property bool busy: false

    signal hovered
    signal clicked

    readonly property color ink: selected ? Tokens.onAccent : Tokens.fg

    implicitHeight: Tokens.powerRowHeight
    opacity: !available || busy ? Tokens.busyOpacity : 1

    Rectangle {
        anchors.fill: parent
        radius: Tokens.radiusRow
        clip: true
        color: row.selected ? Tokens.alpha(Tokens.primary, Tokens.accentFillOpacity) : "transparent"

        Behavior on color {
            ColorAnimation {
                duration: Tokens.durationFast
            }
        }

        Rectangle {
            anchors.bottom: parent.bottom
            visible: row.armed
            width: parent.width * row.armedLeft
            height: Tokens.drainHeight
            color: Tokens.alpha(Tokens.onAccent, Tokens.drainOpacity)
        }
    }

    PowerIcon {
        id: icon
        anchors.verticalCenter: parent.verticalCenter
        x: Tokens.pad + Tokens.labelNudge
        action: row.action
        color: row.ink
    }

    Column {
        anchors.verticalCenter: parent.verticalCenter
        x: icon.x + icon.width + Tokens.pad
        width: parent.width - x - Tokens.pad

        Text {
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            width: parent.width
            elide: Text.ElideRight
            color: row.ink
            text: row.running ? qsTr("%1…").arg(row.doing) : row.armed ? qsTr("%1?").arg(row.label) : row.label
        }

        Text {
            visible: text !== ""
            width: parent.width
            elide: Text.ElideRight
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeTiny
            color: row.selected ? Tokens.alpha(Tokens.onAccent, Tokens.drainOpacity) : Tokens.fgDim
            textFormat: Text.PlainText
            text: row.armed || row.available ? "" : row.reason
        }
    }

    Text {
        anchors.verticalCenter: parent.verticalCenter
        anchors.right: parent.right
        anchors.rightMargin: Tokens.pad
        visible: row.selected && !row.running
        font.family: Tokens.fontMono
        font.pixelSize: Tokens.fontSizeTiny
        color: row.ink
        text: row.armed ? qsTr("%1 s").arg(Math.ceil(row.armedLeft * Tokens.durationConfirm / 1000)) : "↵"
    }

    Spinner {
        anchors.verticalCenter: parent.verticalCenter
        anchors.right: parent.right
        anchors.rightMargin: Tokens.pad
        visible: row.running
        color: row.ink
    }

    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        enabled: row.available && !row.busy
        onEntered: row.hovered()
        onClicked: row.clicked()
    }
}
