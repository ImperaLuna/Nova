import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "listing.js" as Listing

// Nova Files' command row, under the navigation row: New first, then cut,
// copy, paste, rename and delete, then Compress to ZIP, then Sort and View;
// at its right end the preview pane's button, raised while the pane shows
// (Alt+P too). Inside an archive, or with one archive picked, "Extract
// all" comes after New, as in Explorer. View shows the file list's view
// (`view`, a Listing view) and asks for its menu under it.
Rectangle {
    id: row

    property bool previewShown: false
    signal previewToggled

    implicitHeight: actions.implicitHeight + Tokens.filesCommandPadTop + Tokens.gap
    // How solid the window frame is (0..1): the row fades into it.
    property real solid: 0
    // "Extract all" has an archive to extract.
    property bool extractable: false
    // Something is picked that a zip can be made of.
    property bool compressible: false

    signal extractAllRequested
    signal compressRequested

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
        ToolButton {
            visible: row.extractable
            icon: Icons.EXTRACT
            label: qsTr("Extract all")
            onClicked: row.extractAllRequested()
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
        ToolButton {
            icon: Icons.ARCHIVE
            enabled: row.compressible
            onClicked: row.compressRequested()
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
    ToolButton {
        anchors.right: parent.right
        anchors.rightMargin: Tokens.rowInset
        anchors.top: parent.top
        anchors.topMargin: Tokens.filesCommandPadTop
        icon: Icons.PREVIEW_PANE
        label: qsTr("Preview")
        raised: row.previewShown
        onClicked: row.previewToggled()
    }
}
