pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "service"
import "home.js" as Home

// Nova Files' sidebar: the places first (a click goes there): the Home
// page, the folders pinned to Quick access, each with a pin at its end,
// and the trash; the Home page and the trash are marked while the window
// shows them. Then the drives (Drives.qml) under a line of their own, a
// line, then the folder tree that follows the window. A right-click on a
// pinned folder or a tree row asks for the menu that pins or unpins it.
// A drive opened goes through the navigator like a place; when the drive
// the window is on goes away, it goes to the home folder.
Item {
    id: sidebar

    required property Navigator navigator

    // A drive's menu was asked for at `at` (in the window).
    signal driveMenuRequested(var drive, point at)
    // The pin menu of the folder `path` was asked for at `at` (in the
    // window).
    signal pinMenuRequested(string path, point at)

    Column {
        id: placeList
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Tokens.gap
        spacing: Tokens.dividerWidth

        Repeater {
            model: FilesService.places

            Rectangle {
                id: place

                required property var modelData
                readonly property var look: Home.look(modelData)
                readonly property bool pinned: Home.isPinned(modelData)
                // The tree marks folders; only the pages, which aren't in
                // it, are marked here.
                readonly property bool current: Home.isPage(modelData.path) && sidebar.navigator.folder === modelData.path

                width: parent.width
                height: Tokens.filesPlaceHeight
                radius: Tokens.menuRowRadius
                color: current ? Tokens.selected : hover.hovered ? Tokens.hover : "transparent"

                Behavior on color {
                    ColorAnimation {
                        duration: Tokens.durationFast
                    }
                }

                Row {
                    id: placeRow
                    anchors.left: parent.left
                    anchors.right: pin.left
                    anchors.leftMargin: Tokens.gap
                    anchors.rightMargin: Tokens.gap
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: Tokens.rowInset

                    LineIcon {
                        id: placeIcon
                        anchors.verticalCenter: parent.verticalCenter
                        path: place.look.icon
                        color: place.current ? Tokens.primary : Tokens.fg
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        width: placeRow.width - placeIcon.width - placeRow.spacing
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        font.weight: place.current ? Tokens.weightLabel : Tokens.weightBody
                        color: Tokens.fg
                        elide: Text.ElideRight
                        textFormat: Text.PlainText
                        text: place.look.name
                    }
                }
                LineIcon {
                    id: pin
                    anchors.right: parent.right
                    anchors.rightMargin: Tokens.gap
                    anchors.verticalCenter: parent.verticalCenter
                    visible: place.pinned
                    size: Tokens.iconSizeMini
                    color: Tokens.filesFaint
                    path: Icons.PIN
                }

                HoverHandler {
                    id: hover
                }
                TapHandler {
                    onTapped: sidebar.navigator.visit(place.modelData.path, null)
                }
                TapHandler {
                    acceptedButtons: Qt.RightButton
                    enabled: place.pinned
                    onTapped: point => sidebar.pinMenuRequested(place.modelData.path, place.mapToItem(null, point.position))
                }
            }
        }

        Item {
            visible: FilesService.drives.length > 0
            width: parent.width
            height: Tokens.filesRuleInset * 2 + Tokens.dividerWidth

            Rectangle {
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.leftMargin: Tokens.filesRuleInset
                anchors.rightMargin: Tokens.filesRuleInset
                height: Tokens.dividerWidth
                color: Tokens.filesRule
            }
        }
        Drives {
            width: parent.width
            folder: sidebar.navigator.folder
            onOpened: id => sidebar.navigator.openDrive(id)
            onMenuRequested: (drive, at) => sidebar.driveMenuRequested(drive, at)
            onDriveGone: sidebar.navigator.visit("~", null)
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
        onMenuRequested: (path, at) => sidebar.pinMenuRequested(path, at)
    }
}
