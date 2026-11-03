import QtQuick
import "../theme"
import "listing.js" as Listing
import "paths.js" as Paths

// A tile of Nova Files' Icons view: the entry's icon, big, or a file's
// thumbnail in its place once there is one, over its name centered in up
// to two lines (cut short with an ellipsis). Like Details' rows (FileRow),
// hovered tiles lighten, selected ones fill, a folder a drag would drop
// into washes in the accent, dot files read fainter and cut ones faded; a
// press drags, clicks, opens or asks for the menu as a row's does
// (EntryMouse), and while `renaming` its name is a field (NameField).
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
    // A drag over the list would drop into this folder.
    property bool dropTarget: false
    property bool cut: false
    property bool renaming: false
    // How long it waits before easing in, in ms, while a folder's entries
    // have just come in, or -1 to just show: read once, as the tile is made.
    required property int riseWait

    signal pressed(int modifiers)
    // Released where it was pressed, without dragging.
    signal clicked(int modifiers)
    signal dragRequested
    signal opened
    signal middleClicked
    // A right-click at `at` (in the tile).
    signal menuAsked(point at)
    // The name field closed: `name` as typed, or "" for Esc.
    signal renameDone(string name)

    width: Tokens.filesTileWidth
    height: Tokens.filesTileHeight
    radius: Tokens.menuRowRadius
    color: dropTarget ? Tokens.filesDropTarget : selected ? Tokens.selected : mouse.containsMouse ? Tokens.hover : "transparent"
    opacity: cut ? Tokens.filesCutOpacity : 1

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
        visible: !tile.renaming
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

    NameField {
        anchors.top: icon.bottom
        anchors.topMargin: Tokens.iconGap
        anchors.horizontalCenter: parent.horizontalCenter
        width: parent.width - Tokens.filesTilePad
        height: Tokens.filesRowHeight - 2 * Tokens.labelNudge * 2
        alignment: TextInput.AlignHCenter
        name: tile.name
        dir: tile.dir
        renaming: tile.renaming
        onDone: typed => tile.renameDone(typed)
    }

    EntryMouse {
        id: mouse
        anchors.fill: parent
        enabled: !tile.renaming
        onEntryPressed: modifiers => tile.pressed(modifiers)
        onEntryClicked: modifiers => tile.clicked(modifiers)
        onDragRequested: tile.dragRequested()
        onEntryOpened: tile.opened()
        onMiddleClicked: tile.middleClicked()
        onMenuAsked: at => tile.menuAsked(at)
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
