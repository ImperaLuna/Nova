import QtQuick
import "../theme"

// A toolbar or card button: an icon, maybe a label, a hover tint, and an
// accent fill when it's the selected one of a group.
Rectangle {
    id: button

    required property string icon
    property string label: ""
    property bool selected: false
    property color tint: Tokens.fg
    property int buttonHeight: Tokens.captureButtonHeight
    readonly property bool hovered: input.containsMouse
    signal clicked

    implicitWidth: label ? row.implicitWidth + 2 * Tokens.captureButtonPad : buttonHeight
    implicitHeight: buttonHeight
    radius: Tokens.radiusControl
    color: selected ? Tokens.primary : hovered ? Tokens.hover : "transparent"

    Behavior on color {
        ColorAnimation {
            duration: Tokens.durationColor
        }
    }

    Row {
        id: row
        anchors.centerIn: parent
        spacing: Tokens.iconGap

        CaptureIcon {
            anchors.verticalCenter: parent.verticalCenter
            name: button.icon
            color: button.selected ? Tokens.onAccent : button.tint
        }

        Text {
            anchors.verticalCenter: parent.verticalCenter
            visible: button.label !== ""
            text: button.label
            textFormat: Text.PlainText
            color: button.selected ? Tokens.onAccent : button.tint
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            font.weight: Tokens.weightLabel
        }
    }

    MouseArea {
        id: input
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: button.clicked()
    }
}
