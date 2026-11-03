pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "paths.js" as Paths
import "games.js" as Games

// A row of the Games place's list: a gamepad and the game's name, then where
// it's installed and its size, at the header's column widths. Hovered
// rows lighten, the picked one fills.
Rectangle {
    id: item

    required property int index
    required property string name
    required property string folder
    required property real size

    required property real nameWidth
    required property bool selected
    required property string home

    signal pressed
    signal opened
    signal menuRequested(point at)

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
                path: Icons.GAMEPAD
                color: Tokens.fgDim
            }
            Cell {
                width: parent.width - icon.width - parent.spacing
                color: Tokens.fg
                text: item.name
            }
        }
        Cell {
            width: Tokens.filesGamesColumns[0]
            // The game's own folder stays in view; the start gives way.
            elide: Text.ElideLeft
            text: Paths.located(item.folder, item.home)
        }
        Cell {
            width: Tokens.filesGamesColumns[1] - Tokens.rowInset
            horizontalAlignment: Text.AlignRight
            text: Games.size(item)
        }
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onPressed: item.pressed()
        onDoubleClicked: press => {
            if (press.button === Qt.LeftButton)
                item.opened();
        }
        onClicked: press => {
            if (press.button === Qt.RightButton)
                item.menuRequested(mapToItem(null, press.x, press.y));
        }
    }
}
