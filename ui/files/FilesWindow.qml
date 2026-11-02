import QtQuick
import Quickshell
import "../theme"
import "paths.js" as Paths

// A Nova Files window: an app window the compositor tiles and rounds,
// glass all through, showing `folder` with `select` selected. The
// navigation row, the command row, then the sidebar beside the file list,
// with its copies and moves over them all.
FloatingWindow {
    id: window

    required property string folder
    property var select: []

    title: Paths.folderName(folder)
    implicitWidth: Tokens.filesWindowWidth
    implicitHeight: Tokens.filesWindowHeight
    color: Tokens.glass

    // Copies or moves (a Protocol.TRANSFER_* mode) `sources` (absolute
    // paths) into the folder `folder`, showing how it goes in this window.
    function transfer(mode: string, sources: var, folder: string) {
        transfers.start(mode, sources, folder);
    }

    NavigationRow {
        id: navigation
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        folder: window.folder
    }
    CommandRow {
        id: commands
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: navigation.bottom
    }
    Sidebar {
        id: sidebar
        anchors.left: parent.left
        anchors.top: commands.bottom
        anchors.bottom: parent.bottom
        width: Tokens.filesSidebarWidth
    }
    FileList {
        anchors.left: sidebar.right
        anchors.right: parent.right
        anchors.top: commands.bottom
        anchors.bottom: parent.bottom
    }
    Transfers {
        id: transfers
        anchors.fill: parent
    }
}
