import QtQuick
import "../theme"
import "listing.js" as Listing
import "search.js" as Search

// A row of Nova Files' search results: the found file's icon and name
// with the matched words in the accent, then where it lives and its size
// at the results' column widths. Hovered rows lighten, the selected one
// fills; ones that are hidden (or inside a hidden folder) read fainter.
// A click on where it lives shows it there (`locationOpened`).
Rectangle {
    id: result

    // A Search.row.
    required property var modelData
    required property int index
    required property real nameWidth
    required property bool selected
    // Home, which where it lives is shown from (as `~`).
    required property string home

    signal pressed
    signal opened
    signal locationOpened

    height: Tokens.filesRowHeight
    radius: Tokens.menuRowRadius
    color: selected ? Tokens.selected : mouse.containsMouse ? Tokens.hover : "transparent"

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        onPressed: result.pressed()
        onDoubleClicked: result.opened()
    }

    Row {
        anchors.fill: parent
        anchors.leftMargin: Tokens.rowInset
        anchors.rightMargin: Tokens.rowInset
        spacing: Tokens.rowInset

        Row {
            anchors.verticalCenter: parent.verticalCenter
            width: result.nameWidth - Tokens.rowInset
            spacing: Tokens.rowInset

            LineIcon {
                id: icon
                anchors.verticalCenter: parent.verticalCenter
                path: Listing.icon(result.modelData)
                color: result.modelData.dir ? Tokens.filesFolder : Tokens.fgDim
                opacity: result.modelData.dir && result.modelData.dim ? Tokens.filesHiddenIconOpacity : 1
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                width: parent.width - icon.width - parent.spacing
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                color: result.modelData.dim ? Tokens.filesFaint : Tokens.fg
                elide: Text.ElideRight
                textFormat: Text.StyledText
                text: Search.marked(result.modelData.name, result.modelData.marks, Tokens.primary)
            }
        }
        // Where it lives: elided at its start, so the folder it's in shows.
        Text {
            anchors.verticalCenter: parent.verticalCenter
            width: Tokens.filesSearchLocationWidth
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            font.underline: locationHover.hovered
            color: result.modelData.dim ? Tokens.filesFaint : Tokens.fgDim
            elide: Text.ElideLeft
            textFormat: Text.PlainText
            text: Search.location(result.modelData.folder, result.home)

            HoverHandler {
                id: locationHover
                cursorShape: Qt.PointingHandCursor
            }
            TapHandler {
                onTapped: result.locationOpened()
            }
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            width: Tokens.filesColumns[2] - Tokens.rowInset
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            color: Tokens.fgDim
            horizontalAlignment: Text.AlignRight
            textFormat: Text.PlainText
            text: Listing.size(result.modelData)
        }
    }
}
