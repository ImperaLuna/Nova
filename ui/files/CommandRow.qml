pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "commands.js" as Commands

// Nova Files' command row, under the navigation row: New first (its menu
// makes a folder), then cut, copy, paste, rename and move to trash, then
// Sort. Each asks the window (`commandRequested`, a Commands name), or for
// a menu under its button (`newRequested`, `sortRequested`); buttons dim
// while there's nothing for them to do. View waits for the views to choose
// from.
Rectangle {
    id: row

    implicitHeight: actions.implicitHeight + Tokens.filesCommandPadTop + Tokens.gap
    // How solid the window frame is (0..1): the row fades into it.
    property real solid: 0
    // How many items are picked, and whether a paste would bring any.
    property int picked: 0
    property bool canPaste: false

    // `at` is the menu's corner, under the button, in the row.
    signal newRequested(point at)
    signal sortRequested(point at)
    signal commandRequested(string command)

    color: Tokens.alpha(Tokens.surfaceRaised, Tokens.raisedOpacity * (1 - solid))

    component Separator: Rectangle {
        anchors.verticalCenter: parent?.verticalCenter
        width: Tokens.dividerWidth
        height: Tokens.filesSeparatorHeight
        color: Tokens.glassRim
    }

    component Command: ToolButton {
        required property string command
        onClicked: row.commandRequested(command)
    }

    function below(button: Item): point {
        return button.mapToItem(row, 0, button.height);
    }

    Row {
        id: actions
        anchors.left: parent.left
        anchors.leftMargin: Tokens.rowInset
        anchors.top: parent.top
        anchors.topMargin: Tokens.filesCommandPadTop
        spacing: Tokens.labelNudge * 2

        ToolButton {
            id: newButton
            icon: Icons.PLUS
            label: qsTr("New")
            menu: true
            raised: true
            onClicked: row.newRequested(row.below(newButton))
        }
        Separator {}
        Command {
            icon: Icons.CUT
            command: Commands.CUT
            enabled: row.picked > 0
        }
        Command {
            icon: Icons.COPY
            command: Commands.COPY
            enabled: row.picked > 0
        }
        Command {
            icon: Icons.PASTE
            command: Commands.PASTE
            enabled: row.canPaste
        }
        Command {
            icon: Icons.RENAME
            command: Commands.RENAME
            enabled: row.picked === 1
        }
        Command {
            icon: Icons.TRASH
            command: Commands.TRASH
            enabled: row.picked > 0
        }
        Separator {}
        ToolButton {
            id: sortButton
            icon: Icons.SORT
            label: qsTr("Sort")
            menu: true
            onClicked: row.sortRequested(row.below(sortButton))
        }
        ToolButton {
            icon: Icons.VIEW
            label: qsTr("View")
            menu: true
            enabled: false
        }
    }
}
