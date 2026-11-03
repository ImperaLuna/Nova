pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "paths.js" as Paths
import "listing.js" as Listing

// A row of the trash's list: the item's icon and the name it had, then
// where it came from, when it was trashed and its size, at the header's
// column widths. Hovered rows lighten, selected ones fill; dot files read
// fainter.
Rectangle {
    id: item

    required property int index
    required property string name
    required property bool dir
    required property bool hidden
    required property string mime
    required property real size
    required property string folder
    required property real deleted

    required property real nameWidth
    required property bool selected
    // When the Date deleted column was worked out, so "Today" moves on.
    required property real now
    required property string home

    signal pressed(int modifiers)

    height: Tokens.filesRowHeight
    radius: Tokens.menuRowRadius
    color: selected ? Tokens.selected : mouse.containsMouse ? Tokens.hover : "transparent"

    component Cell: Text {
        anchors.verticalCenter: parent?.verticalCenter
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSize
        color: item.hidden ? Tokens.filesFaint : Tokens.fgDim
        elide: Text.ElideRight
        textFormat: Text.PlainText
    }

    Row {
        anchors.fill: parent
        anchors.leftMargin: Tokens.rowInset
        anchors.rightMargin: Tokens.rowInset
        spacing: Tokens.rowInset

        Row {
            anchors.verticalCenter: parent.verticalCenter
            width: item.nameWidth - Tokens.rowInset
            spacing: Tokens.rowInset

            LineIcon {
                id: icon
                anchors.verticalCenter: parent.verticalCenter
                path: Listing.icon(item)
                color: item.dir ? Tokens.filesFolder : Tokens.fgDim
                opacity: item.dir && item.hidden ? Tokens.filesHiddenIconOpacity : 1
            }
            Cell {
                width: parent.width - icon.width - parent.spacing
                color: item.hidden ? Tokens.filesFaint : Tokens.fg
                text: Paths.shown(item.name)
            }
        }
        Cell {
            width: Tokens.filesTrashColumns[0]
            // The folder's own name stays in view; the start gives way.
            elide: Text.ElideLeft
            text: Paths.location(item.folder, item.home)
        }
        Cell {
            width: Tokens.filesTrashColumns[1]
            text: Listing.modified(item.deleted, item.now)
        }
        Cell {
            width: Tokens.filesTrashColumns[2] - Tokens.rowInset
            horizontalAlignment: Text.AlignRight
            text: Listing.size(item)
        }
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        onPressed: press => item.pressed(press.modifiers)
    }
}
