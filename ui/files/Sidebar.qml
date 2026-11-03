pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol
import "service"
import "drops.js" as Drops
import "trash.js" as Trash

// Nova Files' sidebar: the places that exist first (a click goes there),
// the trash last among them and marked while the window shows it, then
// the drives (Drives.qml) under a line of their own, a line, then the
// folder tree that follows the window. A drive opened goes through the
// navigator like a place; when the drive the window is on goes away, it
// goes home. Files dragged onto a place, a mounted drive or a folder in
// the tree drop there (onto the trash, into the trash), and a tree folder
// drags out; the places themselves stay put.
Item {
    id: sidebar

    required property Navigator navigator
    // The places novad found ({kind, path}), in order.
    property var places: []

    // A drive's menu was asked for at `at` (in the window).
    signal driveMenuRequested(var drive, point at)
    // A drop asks for `action` (a Protocol.TRANSFER_* mode or Drops.TRASH) on `sources` into `folder`.
    signal dropRequested(string action, var sources, string folder)

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
            },
            [Protocol.PLACE_TRASH]: {
                name: Trash.name(),
                icon: Icons.TRASH
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
                // The tree marks folders; only the trash, which isn't in
                // it, is marked here.
                readonly property bool current: Trash.isTrash(modelData.path) && sidebar.navigator.folder === modelData.path

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
                    anchors.left: parent.left
                    anchors.leftMargin: Tokens.gap
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: Tokens.rowInset

                    LineIcon {
                        anchors.verticalCenter: parent.verticalCenter
                        path: place.look.icon
                        color: place.current ? Tokens.primary : Tokens.fg
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        font.weight: place.current ? Tokens.weightLabel : Tokens.weightBody
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
            onOpened: folder => sidebar.navigator.visit(folder, null)
            onMenuRequested: (drive, at) => sidebar.driveMenuRequested(drive, at)
            onDriveGone: sidebar.navigator.visit("~", null)
            onDropRequested: (action, sources, folder) => sidebar.dropRequested(action, sources, folder)
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
