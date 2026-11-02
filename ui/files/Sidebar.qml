pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "service"

// Nova Files' sidebar: the places first, then the drives (Drives.qml).
// The folder tree comes between them. `folder` is the window's.
Item {
    id: sidebar

    property string folder: ""

    // A drive was opened at `folder`, or its menu asked for at `at` (in the
    // window).
    signal opened(string folder)
    signal driveMenuRequested(var drive, point at)
    // The drive the window's folder is on went away.
    signal driveLeft

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

        Rectangle {
            visible: FilesService.drives.length > 0
            width: parent.width - 2 * Tokens.menuSeparatorInset
            x: Tokens.menuSeparatorInset
            height: Tokens.dividerWidth
            color: Tokens.glassRim
        }
        Item {
            visible: FilesService.drives.length > 0
            width: parent.width
            height: Tokens.menuSeparatorGap
        }
        Drives {
            width: parent.width
            folder: sidebar.folder
            onOpened: folder => sidebar.opened(folder)
            onMenuRequested: (drive, at) => sidebar.driveMenuRequested(drive, at)
            onLeft: sidebar.driveLeft()
        }
    }
}
