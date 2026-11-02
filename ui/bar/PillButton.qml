import QtQuick
import "../theme"

// A round button on the right pill (and in the same place on quick
// settings): a line icon, a hover tint, and a quiet fill while `picked`.
Rectangle {
    id: button

    required property string icon
    property color ink: Tokens.fg
    property bool picked: false
    signal clicked

    width: Tokens.pillButtonSize
    height: Tokens.pillButtonSize
    radius: width / 2
    color: picked ? Tokens.selectedQuiet : press.containsMouse ? Tokens.hover : "transparent"

    Behavior on color {
        ColorAnimation {
            duration: Tokens.durationFast
        }
    }

    LineIcon {
        anchors.centerIn: parent
        path: button.icon
        color: button.ink
    }

    MouseArea {
        id: press
        anchors.fill: parent
        hoverEnabled: true
        onClicked: button.clicked()
    }
}
