pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "drops.js" as Drops

// Drags Nova Files items: their paths go as text/uri-list, so a Nova Files
// window, Firefox's upload box, Discord or the desktop all take them, as
// a copy or a move (the target decides, Ctrl asks for a copy). The pointer
// carries a tile with the item's icon and, for more than one, how many.
// One per list rather than per row: the drag outlives the row it started
// on, which a folder opened by hovering replaces.
Item {
    id: dragger

    // Starts dragging `paths` (absolute), showing `icon` in `iconColor`.
    function start(paths: var, icon: string, iconColor: color) {
        if (paths.length === 0 || dragger.Drag.active)
            return;
        tile.count = paths.length;
        tile.icon = icon;
        tile.iconColor = iconColor;
        Drag.mimeData = {
            [Drops.URI_LIST]: Drops.uriList(paths)
        };
        tile.grabToImage(grabbed => {
            dragger.Drag.imageSource = grabbed.url;
            dragger.Drag.active = true;
        });
    }

    Drag.dragType: Drag.Automatic
    Drag.supportedActions: Qt.CopyAction | Qt.MoveAction
    Drag.proposedAction: Qt.MoveAction
    Drag.hotSpot.x: Tokens.filesDragTile / 2
    Drag.hotSpot.y: Tokens.filesDragTile / 2

    // The tile under the pointer. Drawn out of sight: it's only grabbed.
    Item {
        id: tile

        property int count: 0
        property string icon: ""
        property color iconColor: Tokens.fg

        x: -width - Tokens.filesDragTile
        width: Tokens.filesDragTile + Tokens.filesDragCount / 2
        height: width

        Rectangle {
            anchors.left: parent.left
            anchors.bottom: parent.bottom
            width: Tokens.filesDragTile
            height: Tokens.filesDragTile
            radius: Tokens.radiusRow
            color: Tokens.filesSolidFrame
            border.width: Tokens.dividerWidth
            border.color: Tokens.glassRim

            LineIcon {
                anchors.centerIn: parent
                size: Tokens.filesDragIcon
                path: tile.icon === "" ? "M0 0" : tile.icon
                color: tile.iconColor
            }
        }
        Rectangle {
            anchors.right: parent.right
            anchors.top: parent.top
            visible: tile.count > 1
            width: Math.max(height, countText.implicitWidth + Tokens.filesDragCount / 2)
            height: Tokens.filesDragCount
            radius: height / 2
            color: Tokens.primary

            Text {
                id: countText
                anchors.centerIn: parent
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeSmall
                font.weight: Tokens.weightLabel
                color: Tokens.onAccent
                text: tile.count
            }
        }
    }
}
