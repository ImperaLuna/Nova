pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol
import "service"

// Nova Files' sidebar: the places that exist first (a click goes there),
// a line, then the folder tree that follows the window. The drives come
// under it.
Item {
    id: sidebar

    required property Navigator navigator
    // The places novad found ({kind, path}), in order.
    property var places: []

    readonly property var looks: ({
            [Protocol.PLACE_HOME]: {
                name: qsTr("Home"),
                icon: Icons.HOME
            },
            [Protocol.PLACE_DESKTOP]: {
                name: qsTr("Desktop"),
                icon: Icons.DESKTOP
            },
            [Protocol.PLACE_DOCUMENTS]: {
                name: qsTr("Documents"),
                icon: Icons.DOCUMENT
            },
            [Protocol.PLACE_DOWNLOADS]: {
                name: qsTr("Downloads"),
                icon: Icons.DOWNLOAD
            },
            [Protocol.PLACE_PICTURES]: {
                name: qsTr("Pictures"),
                icon: Icons.PICTURE
            }
        })

    Component.onCompleted: FilesService.places(found => sidebar.places = found)

    Column {
        id: placeList
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Tokens.gap
        spacing: Tokens.dividerWidth

        Repeater {
            model: sidebar.places

            Rectangle {
                id: place

                required property var modelData
                readonly property var look: sidebar.looks[modelData.kind]

                width: parent.width
                height: Tokens.filesPlaceHeight
                radius: Tokens.menuRowRadius
                color: hover.hovered ? Tokens.hover : "transparent"

                Behavior on color {
                    ColorAnimation {
                        duration: Tokens.durationFast
                    }
                }

                Row {
                    anchors.left: parent.left
                    anchors.leftMargin: Tokens.gap
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: Tokens.rowInset

                    LineIcon {
                        anchors.verticalCenter: parent.verticalCenter
                        path: place.look.icon
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        color: Tokens.fg
                        text: place.look.name
                    }
                }

                HoverHandler {
                    id: hover
                }
                TapHandler {
                    onTapped: sidebar.navigator.visit(place.modelData.path, null)
                }
            }
        }
    }

    Rectangle {
        id: rule
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: placeList.bottom
        anchors.leftMargin: Tokens.gap + Tokens.filesRuleInset
        anchors.rightMargin: Tokens.gap + Tokens.filesRuleInset
        anchors.topMargin: Tokens.filesRuleInset
        height: Tokens.dividerWidth
        color: Tokens.filesRule
    }

    FolderTree {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: rule.bottom
        anchors.bottom: parent.bottom
        anchors.leftMargin: Tokens.gap
        anchors.rightMargin: Tokens.gap
        anchors.topMargin: Tokens.filesRuleInset
        anchors.bottomMargin: Tokens.gap
        navigator: sidebar.navigator
    }
}
