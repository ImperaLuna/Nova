import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// A page's top row: a round back button and the page's title.
Item {
    id: header

    required property string title

    signal back

    implicitHeight: Tokens.logoSize

    Rectangle {
        id: button
        anchors.verticalCenter: parent.verticalCenter
        width: Tokens.pillButtonSize + Tokens.labelNudge * 2
        height: width
        radius: width / 2
        color: press.containsMouse ? Tokens.hover : Tokens.surfaceRaised

        LineIcon {
            anchors.centerIn: parent
            path: Icons.LEFT
        }

        MouseArea {
            id: press
            anchors.fill: parent
            hoverEnabled: true
            onClicked: header.back()
        }
    }

    Text {
        anchors.verticalCenter: parent.verticalCenter
        x: button.width + Tokens.gap
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeHeading
        font.weight: Tokens.weightTitle
        color: Tokens.fg
        text: header.title
    }
}
