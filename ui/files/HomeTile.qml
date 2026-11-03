import QtQuick
import "../theme"
import "space.js" as Space

// A tile on Nova Files' Home page, as Explorer's Quick access and drives
// show them: a big icon beside a name and, under it, a fainter line (where
// the folder is) or, for a drive, how full it is (DriveSpace). Hovered tiles lighten, the selected one fills. A click
// selects it, a double-click opens it, a right-click asks for its menu at
// the pointer (in the window).
Rectangle {
    id: tile

    required property string icon
    property color iconColor: Tokens.fg
    required property string title
    property string detail: ""
    // A drive (novad's), whose bar and free space show under the title.
    property var drive: null
    property bool selected: false

    signal picked
    signal opened
    signal menuRequested(point at)

    width: Tokens.filesHomeTileWidth
    height: Tokens.filesHomeTileHeight
    radius: Tokens.menuRowRadius
    color: selected ? Tokens.selected : mouse.containsMouse ? Tokens.hover : "transparent"

    Behavior on color {
        ColorAnimation {
            duration: Tokens.durationFast
        }
    }

    Row {
        id: content
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Tokens.rowInset
        anchors.rightMargin: Tokens.rowInset
        anchors.verticalCenter: parent.verticalCenter
        spacing: Tokens.rowInset

        LineIcon {
            id: glyph
            anchors.verticalCenter: parent.verticalCenter
            size: Tokens.filesHomeTileIconSize
            color: tile.iconColor
            path: tile.icon
        }
        Column {
            anchors.verticalCenter: parent.verticalCenter
            width: content.width - glyph.width - content.spacing
            spacing: Tokens.filesDriveBarGap

            Text {
                width: parent.width
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                font.weight: Tokens.weightLabel
                color: Tokens.fg
                elide: Text.ElideRight
                textFormat: Text.PlainText
                text: tile.title
            }
            DriveSpace {
                visible: tile.drive !== null && Space.measured(tile.drive)
                width: parent.width
                drive: tile.drive ?? ({})
            }
            Text {
                visible: tile.detail !== ""
                width: parent.width
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeSmall
                color: Tokens.filesFaint
                elide: Text.ElideMiddle
                textFormat: Text.PlainText
                text: tile.detail
            }
        }
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onPressed: press => {
            tile.picked();
            if (press.button === Qt.RightButton)
                tile.menuRequested(tile.mapToItem(null, press.x, press.y));
        }
        onDoubleClicked: press => {
            if (press.button === Qt.LeftButton)
                tile.opened();
        }
    }
}
