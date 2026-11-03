pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "service"
import "drops.js" as Drops
import "home.js" as Home

// Nova Files' sidebar: the places first (a click goes there): the Home
// page, the folders pinned to Quick access, each with a pin at its end,
// and the trash; the Home page and the trash are marked while the window
// shows them. Then the drives (Drives.qml) under a line of their own, a
// line, then the folder tree that follows the window. A right-click on a
// pinned folder or a tree row asks for the menu that pins or unpins it.
// A drive clicked is opened by the window (`driveOpened`), showing what
// uses space on it; when the drive the window is on goes away, it goes to
// the home folder. Files dragged onto a place, a mounted drive or a folder
// in the tree drop there (onto the trash, into the trash), and a tree
// folder drags out; the places themselves stay put. A middle click on a
// place or a tree folder asks for it in a new tab (`tabRequested`).
Item {
    id: sidebar

    required property Navigator navigator

    // A drive's menu was asked for at `at` (in the window).
    signal driveMenuRequested(var drive, point at)
    signal tabRequested(string path)
    // The pin menu of the folder `path` was asked for at `at` (in the
    // window).
    signal pinMenuRequested(string path, point at)
    // The drive `id` was clicked: the window opens it.
    signal driveOpened(string id)
    // A drop asks for `action` (a Protocol.TRANSFER_* mode or Drops.TRASH) on `sources` into `folder`.
    signal dropRequested(string action, var sources, string folder)

    // The places and drives scroll once they'd take more than their share
    // of the sidebar (many pins), so the tree under them keeps its room.
    Flickable {
        id: placeScroll
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Tokens.gap
        height: Math.min(placeList.implicitHeight, sidebar.height * Tokens.filesPlacesMaxShare)
        contentHeight: placeList.implicitHeight
        boundsBehavior: Flickable.StopAtBounds
        clip: true

        Column {
            id: placeList
            width: placeScroll.width
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
                    color: placeDrops.action !== Drops.NONE ? Tokens.filesDropTarget : current ? Tokens.selected : hover.hovered ? Tokens.hover : "transparent"

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
                    TapHandler {
                        acceptedButtons: Qt.MiddleButton
                        onTapped: sidebar.tabRequested(place.modelData.path)
                    }
                    DropTarget {
                        id: placeDrops
                        anchors.fill: parent
                        service: FilesService
                        folderAt: () => Drops.place(place.modelData.path, false)
                        onDropRequested: (action, sources, folder) => sidebar.dropRequested(action, sources, folder)
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
                onOpened: id => sidebar.driveOpened(id)
                onMenuRequested: (drive, at) => sidebar.driveMenuRequested(drive, at)
                onDriveGone: sidebar.navigator.visit("~", null)
                onDropRequested: (action, sources, folder) => sidebar.dropRequested(action, sources, folder)
            }
        }
    }

    Rectangle {
        id: rule
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: placeScroll.bottom
        anchors.leftMargin: Tokens.gap + Tokens.filesRuleInset
        anchors.rightMargin: Tokens.gap + Tokens.filesRuleInset
        anchors.topMargin: Tokens.filesRuleInset
        height: Tokens.dividerWidth
        color: Tokens.filesRule
    }

    FolderTree {
        id: tree
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: rule.bottom
        anchors.bottom: parent.bottom
        anchors.leftMargin: Tokens.gap
        anchors.rightMargin: Tokens.gap
        anchors.topMargin: Tokens.filesRuleInset
        anchors.bottomMargin: Tokens.gap
        navigator: sidebar.navigator
        onTabRequested: path => sidebar.tabRequested(path)
        onMenuRequested: (path, at) => sidebar.pinMenuRequested(path, at)
        dropFolder: treeDrops.action !== Drops.NONE ? treeDrops.hovered.folder : ""
        onDragRequested: path => treeDrag.start([path], Icons.FOLDER, Tokens.filesFolder)
    }
    DropTarget {
        id: treeDrops
        anchors.fill: tree
        service: FilesService
        folderAt: (x, y) => tree.placeAt(x, y)
        onDropRequested: (action, sources, folder) => sidebar.dropRequested(action, sources, folder)
        onSpringOpened: folder => sidebar.navigator.toggle(folder, false)
    }
    FileDrag {
        id: treeDrag
    }
}
