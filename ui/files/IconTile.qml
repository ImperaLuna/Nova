import QtQuick
import "../theme"
import "listing.js" as Listing
import "paths.js" as Paths

// A tile of Nova Files' Icons view: the entry's icon, big, over its name
// centered in up to two lines (cut short with an ellipsis). Hovered tiles
// lighten, selected ones fill, like Details' rows; dot files read fainter.
// When a folder's entries have just come in (`riseOrder`), it eases up
// into place after the tiles nearer where the list opened.
Rectangle {
    id: tile

    required property int index
    required property string name
    required property bool dir
    required property bool hidden
    required property string mime

    required property bool selected
    // Steps from where the list opened, while a folder's entries have just
    // come in, or -1: read once, as the tile is made.
    required property int riseOrder

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

    Component.onCompleted: {
        if (riseOrder >= 0)
            riseIn.start();
    }

    RiseIn {
        id: riseIn
        item: tile
        lift: tileLift
        order: tile.riseOrder
    }
}
