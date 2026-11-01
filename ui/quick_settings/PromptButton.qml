import QtQuick
import "../theme"

// A small button on a question card: quiet, or filled with the accent for
// the answer that goes ahead. Dimmed while it can't be pressed.
Rectangle {
    id: button

    property alias text: label.text
    property bool accent: false

    signal clicked

    width: label.implicitWidth + 2 * Tokens.pad
    height: Tokens.confirmButtonHeight
    radius: Tokens.radiusSegment
    opacity: enabled ? 1 : Tokens.dimOpacity
    color: accent ? Tokens.primary : press.containsMouse ? Tokens.hover : Tokens.track

    Text {
        id: label
        anchors.centerIn: parent
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeSmall
        font.weight: button.accent ? Font.DemiBold : Font.Normal
        color: button.accent ? Tokens.onAccent : Tokens.fg
    }
    MouseArea {
        id: press
        anchors.fill: parent
        hoverEnabled: true
        onClicked: button.clicked()
    }
}
