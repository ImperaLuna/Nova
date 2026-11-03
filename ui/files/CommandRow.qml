import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "listing.js" as Listing

// Nova Files' command row, under the navigation row: New first, then cut,
// copy, paste, rename and delete, then Sort and View. View shows the file
// list's view (`view`, a Listing view) and asks for its menu under it.
Rectangle {
    id: row

    implicitHeight: actions.implicitHeight + Tokens.filesCommandPadTop + Tokens.gap
    // How solid the window frame is (0..1): the row fades into it.
    property real solid: 0
    property string view: Listing.DETAILS

    // View was clicked: its menu goes at `at`, in the row's coordinates.
    signal viewMenuRequested(point at)

    color: Tokens.alpha(Tokens.surfaceRaised, Tokens.raisedOpacity * (1 - solid))

    component Separator: Rectangle {
        anchors.verticalCenter: parent?.verticalCenter
        width: Tokens.dividerWidth
        height: Tokens.filesSeparatorHeight
        color: Tokens.glassRim
    }

    Row {
        id: actions
        anchors.left: parent.left
        anchors.leftMargin: Tokens.rowInset
        anchors.top: parent.top
        anchors.topMargin: Tokens.filesCommandPadTop
        spacing: Tokens.labelNudge * 2

        ToolButton {
            icon: Icons.PLUS
            label: qsTr("New")
            menu: true
            raised: true
            enabled: false
        }
        Separator {}
        ToolButton {
            icon: Icons.CUT
            enabled: false
        }
        ToolButton {
            icon: Icons.COPY
            enabled: false
        }
        ToolButton {
            icon: Icons.PASTE
            enabled: false
        }
        ToolButton {
            icon: Icons.RENAME
            enabled: false
        }
        ToolButton {
            icon: Icons.TRASH
            enabled: false
        }
        Separator {}
        ToolButton {
            icon: Icons.SORT
            label: qsTr("Sort")
            menu: true
            enabled: false
        }
        ToolButton {
            id: viewButton
            icon: row.view === Listing.ICONS ? Icons.GRID : Icons.VIEW
            label: qsTr("View")
            menu: true
            onClicked: row.viewMenuRequested(viewButton.mapToItem(row, 0, viewButton.height + Tokens.labelNudge * 2))
        }
    }
}
