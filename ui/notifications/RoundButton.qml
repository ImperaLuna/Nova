import QtQuick
import "../theme"

// A small round icon button, raised: the list's ✕, ⋯, bell and gear. `on`
// fills it with the accent (do not disturb).
Rectangle {
    id: button

    required property string icon
    // Solid parts of the icon (LineIcon `fill`).
    property string fill: ""
    property bool on: false
    property int size: Tokens.notificationButton
    // Faint until hovered, for buttons that sit on a card.
    property bool quiet: false

    signal clicked

    implicitWidth: size
    implicitHeight: size
    radius: size / 2
    color: on ? Tokens.primary : press.containsMouse ? Tokens.hover : quiet ? "transparent" : Tokens.surfaceRaised

    LineIcon {
        anchors.centerIn: parent
        path: button.icon
        fill: button.fill
        size: Tokens.iconSizeSmall
        color: button.on ? Tokens.onAccent : Tokens.fg
    }

    MouseArea {
        id: press
        anchors.fill: parent
        hoverEnabled: true
        onClicked: button.clicked()
    }
}
