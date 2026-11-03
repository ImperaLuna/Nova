import QtQuick
import "../theme"
import "listing.js" as Listing
import "paths.js" as Paths

// A row of the Home page's Recent list: the file's icon and name, then
// when it was used and where it is, at the header's column widths. Hovered
// rows lighten, the selected one fills. A click selects it, a double-click
// opens it.
Rectangle {
    id: entry

    // A Home.recentRow.
    required property var file
    required property bool selected
    // When the Date used column was worked out, so "Today" moves on.
    required property real now
    // The home folder, which locations start from as `~`.
    required property string home

    signal picked
    signal opened

    height: Tokens.filesRowHeight
    radius: Tokens.menuRowRadius
    color: selected ? Tokens.selected : mouse.containsMouse ? Tokens.hover : "transparent"

    component Cell: Text {
        anchors.verticalCenter: parent?.verticalCenter
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSize
        color: Tokens.fgDim
        elide: Text.ElideRight
        textFormat: Text.PlainText
    }

    Row {
        id: cells
        anchors.fill: parent
        anchors.leftMargin: Tokens.rowInset
        anchors.rightMargin: Tokens.rowInset
        spacing: Tokens.rowInset

        Row {
            anchors.verticalCenter: parent.verticalCenter
            width: cells.width - Tokens.filesRecentColumns[0] - Tokens.filesRecentColumns[1] - 2 * cells.spacing
            spacing: Tokens.rowInset

            LineIcon {
                id: icon
                anchors.verticalCenter: parent.verticalCenter
                path: Listing.icon(entry.file)
                color: entry.file.dir ? Tokens.filesFolder : Tokens.fgDim
            }
            Cell {
                width: parent.width - icon.width - parent.spacing
                color: entry.file.hidden ? Tokens.filesFaint : Tokens.fg
                text: Paths.shown(entry.file.name)
            }
        }
        Cell {
            width: Tokens.filesRecentColumns[0]
            text: Listing.modified(entry.file.used, entry.now)
        }
        Cell {
            width: Tokens.filesRecentColumns[1]
            elide: Text.ElideMiddle
            text: Paths.located(entry.file.folder, entry.home)
        }
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        onPressed: entry.picked()
        onDoubleClicked: entry.opened()
    }
}
