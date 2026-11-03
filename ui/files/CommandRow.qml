pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "commands.js" as Commands
import "listing.js" as Listing

// Nova Files' command row, under the navigation row: New first (its menu
// makes a folder), then cut, copy, paste, rename and move to trash, then
// Compress to ZIP, then Sort and View, then What uses space. Inside an
// archive, or with one archive picked, "Extract all" comes after New, as
// in Explorer. Each asks the window (`commandRequested`, a Commands name),
// or for a menu under its button (`newRequested`, `sortRequested`,
// `viewMenuRequested`); buttons dim while there's nothing for them to do.
// View shows the file list's view (`view`, a Listing view). What uses
// space stays lit while the list shows it (`spaceView`). At its right end
// the preview pane's button, raised while the pane shows (Alt+P too).
Rectangle {
    id: row

    implicitHeight: actions.implicitHeight + Tokens.filesCommandPadTop + Tokens.gap
    // How solid the window frame is (0..1): the row fades into it.
    property real solid: 0
    // How many items are picked, and whether a paste would bring any.
    property int picked: 0
    property bool canPaste: false
    // The preview pane shows: its button is raised.
    property bool previewShown: false

    // `at` is the menu's corner, under the button, in the row.
    signal newRequested(point at)
    signal sortRequested(point at)
    signal commandRequested(string command)
    // "Extract all" has an archive to extract.
    property bool extractable: false
    // Something is picked that a zip can be made of.
    property bool compressible: false

    property string view: Listing.DETAILS

    // View was clicked: its menu goes at `at`, in the row's coordinates.
    signal viewMenuRequested(point at)

    property bool spaceView: false

    signal spaceToggled

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
        Command {
            visible: row.extractable
            icon: Icons.EXTRACT
            label: qsTr("Extract all")
            command: Commands.EXTRACT_ALL
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
        Command {
            icon: Icons.ARCHIVE
            command: Commands.COMPRESS
            enabled: row.compressible
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
            id: viewButton
            icon: row.view === Listing.ICONS ? Icons.GRID : Icons.VIEW
            label: qsTr("View")
            menu: true
            onClicked: row.viewMenuRequested(viewButton.mapToItem(row, 0, viewButton.height + Tokens.labelNudge * 2))
        }
        Separator {}
        ToolButton {
            icon: Icons.SIZES
            label: qsTr("What uses space")
            checked: row.spaceView
            onClicked: row.spaceToggled()
        }
    }
    Command {
        anchors.right: parent.right
        anchors.rightMargin: Tokens.rowInset
        anchors.top: parent.top
        anchors.topMargin: Tokens.filesCommandPadTop
        icon: Icons.PREVIEW_PANE
        label: qsTr("Preview")
        command: Commands.PREVIEW
        raised: row.previewShown
    }
}
