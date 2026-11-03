import QtQuick
import "../theme"
import "listing.js" as Listing
import "paths.js" as Paths

// A tile of Nova Files' Icons view: the entry's icon, big, or a file's
// thumbnail in its place once there is one, over its name centered in up
// to two lines (cut short with an ellipsis). Hovered tiles lighten,
// selected ones fill, like Details' rows; dot files read fainter.
// When a folder's entries have just come in (`riseWait`), it eases up
// into place after the tiles nearer where the list opened.
Rectangle {
    id: tile

    required property int index
    required property string name
    required property bool dir
    required property bool hidden
    required property string mime
    required property real modified
    // The folder it's in, for the thumbnail.
    required property string folder
    // What thumbnails are asked of (FileThumbnail.thumbnails).
    required property var thumbnails

    required property bool selected
    // How long it waits before easing in, in ms, while a folder's entries
    // have just come in, or -1 to just show: read once, as the tile is made.
    required property int riseWait

    signal pressed(int modifiers)
    signal opened

    width: Tokens.filesTileWidth
    height: Tokens.filesTileHeight
    radius: Tokens.menuRowRadius
    color: selected ? Tokens.selected : mouse.containsMouse ? Tokens.hover : "transparent"

    LineIcon {
        id: icon
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top
        anchors.topMargin: Tokens.filesTilePad
        size: Tokens.filesTileIconSize
        stroke: Tokens.filesTileIconStroke
        path: Listing.icon(tile)
        color: tile.dir ? Tokens.filesFolder : Tokens.fgDim
        opacity: tile.dir && tile.hidden ? Tokens.filesHiddenIconOpacity : 1
        visible: !thumbnail.shown
    }
    // As wide as the tile allows, as tall as the icon: the name stays put.
    FileThumbnail {
        id: thumbnail
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: icon.top
        width: parent.width - 2 * Tokens.filesTilePad
        height: icon.height
        path: tile.dir ? "" : Paths.child(tile.folder, tile.name)
        modified: tile.modified
        thumbnails: tile.thumbnails
    }
    Text {
        anchors.top: icon.bottom
        anchors.topMargin: Tokens.iconGap
        anchors.horizontalCenter: parent.horizontalCenter
        width: parent.width - 2 * Tokens.filesTilePad
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.WrapAtWordBoundaryOrAnywhere
        maximumLineCount: Tokens.filesTileNameLines
        elide: Text.ElideRight
        textFormat: Text.PlainText
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSize
        color: tile.hidden ? Tokens.filesFaint : Tokens.fg
        text: Paths.shown(tile.name)
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        onPressed: press => tile.pressed(press.modifiers)
        onDoubleClicked: tile.opened()
    }

    transform: Translate {
        id: tileLift
    }

    RiseIn {
        item: tile
        lift: tileLift
        wait: tile.riseWait
    }
}
