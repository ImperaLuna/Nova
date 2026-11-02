pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// Nova Files' sidebar: the places first. The folder tree and the drives
// come under them.
Item {
    id: sidebar

    readonly property var places: [
        {
            name: qsTr("Home"),
            icon: Icons.HOME
        },
        {
            name: qsTr("Desktop"),
            icon: Icons.DESKTOP
        },
        {
            name: qsTr("Documents"),
            icon: Icons.DOCUMENT
        },
        {
            name: qsTr("Downloads"),
            icon: Icons.DOWNLOAD
        },
        {
            name: qsTr("Pictures"),
            icon: Icons.PICTURE
        }
    ]

    Column {
        anchors.fill: parent
        anchors.margins: Tokens.gap
        spacing: Tokens.dividerWidth

        Repeater {
            model: sidebar.places

            Rectangle {
                id: place

                required property var modelData

                width: parent.width
                height: Tokens.filesPlaceHeight
                radius: Tokens.menuRowRadius
                color: hover.hovered ? Tokens.hover : "transparent"

                Row {
                    anchors.left: parent.left
                    anchors.leftMargin: Tokens.gap
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: Tokens.rowInset

                    LineIcon {
                        anchors.verticalCenter: parent.verticalCenter
                        path: place.modelData.icon
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        color: Tokens.fg
                        text: place.modelData.name
                    }
                }

                HoverHandler {
                    id: hover
                }
            }
        }
    }
}
