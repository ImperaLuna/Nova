import QtQuick
import Quickshell
import "../theme"
import "service"
import "paths.js" as Paths

// A Nova Files window: an app window the compositor tiles and rounds,
// showing `folder` with `select` selected. The navigation row, the command
// row, then the sidebar beside the file list; opening a folder in the
// list shows it in the same window. `folder` follows the window's
// Navigator as it moves (Alt+Left, Alt+Right, Alt+Up and the mouse's back
// and forward buttons too). Its frame and its file list each go from
// glass to solid as config.toml's [files] says, live. Each draws its own
// background, so one never shows through the other.
FloatingWindow {
    id: window

    required property string folder
    property var select: []
    readonly property Navigator navigator: Navigator {
        hidden: list.showHidden
        onFolderChanged: window.folder = folder
    }
    readonly property color frameColor: Tokens.mix(Tokens.glass, Tokens.filesSolidFrame, FilesService.frameSolid)

    title: Paths.folderName(folder)
    implicitWidth: Tokens.filesWindowWidth
    implicitHeight: Tokens.filesWindowHeight
    color: "transparent"

    Component.onCompleted: navigator.begin(folder)

    Shortcut {
        sequence: "Alt+Left"
        onActivated: window.navigator.step(-1)
    }
    Shortcut {
        sequence: "Alt+Right"
        onActivated: window.navigator.step(1)
    }
    Shortcut {
        sequence: "Alt+Up"
        onActivated: window.navigator.up()
    }
    Shortcut {
        sequences: ["Ctrl+L", "Alt+D"]
        onActivated: navigation.editAddress()
    }
    TapHandler {
        acceptedButtons: Qt.BackButton | Qt.ForwardButton
        onTapped: (point, button) => window.navigator.step(button === Qt.BackButton ? -1 : 1)
    }

    // The frame's background: behind the two rows, and the sidebar.
    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: commands.bottom
        color: window.frameColor
    }
    Rectangle {
        anchors.fill: sidebar
        color: window.frameColor
    }
    NavigationRow {
        id: navigation
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        navigator: window.navigator
        solid: FilesService.frameSolid
        onAddressClosed: list.forceActiveFocus()
    }
    CommandRow {
        id: commands
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: navigation.bottom
        solid: FilesService.frameSolid
    }
    Sidebar {
        id: sidebar
        anchors.left: parent.left
        anchors.top: commands.bottom
        anchors.bottom: parent.bottom
        width: Tokens.filesSidebarWidth
        navigator: window.navigator
    }
    FileList {
        id: list
        anchors.left: sidebar.right
        anchors.right: parent.right
        anchors.top: commands.bottom
        anchors.bottom: parent.bottom
        folder: window.folder
        select: window.select
        focus: true
        onFolderOpened: path => window.navigator.visit(path, null)
        solid: FilesService.listSolid
    }
}
