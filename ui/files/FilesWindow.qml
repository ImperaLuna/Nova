import QtQuick
import Quickshell
import "../theme"
import "paths.js" as Paths
import "service"
import "../daemon/protocol.js" as Protocol

// A Nova Files window: an app window the compositor tiles and rounds,
// glass all through, showing `folder` with `select` selected. The
// navigation row, the command row, then the sidebar beside the file list.
// A drive's menu and the dialog when one can't be ejected lie over it all.
FloatingWindow {
    id: window

    required property string folder
    property var select: []

    title: Paths.folderName(folder)
    implicitWidth: Tokens.filesWindowWidth
    implicitHeight: Tokens.filesWindowHeight
    color: Tokens.glass

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
        folder: window.folder
        onOpened: folder => window.folder = folder
        onDriveMenuRequested: (drive, at) => driveMenu.open(drive, at)
    }
    FileList {
        anchors.left: sidebar.right
        anchors.right: parent.right
        anchors.top: commands.bottom
        anchors.bottom: parent.bottom
    }
    DriveMenu {
        id: driveMenu
        anchors.fill: parent
        // Once it's ejected the window leaves it, for Home.
        onEjectChosen: drive => FilesService.eject(drive.id, outcome => {
            if (outcome === Protocol.EJECT_BUSY)
                ejectProblem.show(drive.name);
            else if (outcome === Protocol.EJECT_EJECTED && Paths.inside(window.folder, drive.mount ?? ""))
                window.folder = Quickshell.env("HOME") ?? "/";
        })
    }
    EjectProblem {
        id: ejectProblem
        anchors.fill: parent
    }
}
