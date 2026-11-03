import QtQuick
import "../theme"

// A tile on Nova Files' Home page, as Explorer's Quick access and drives
// show them: a big icon beside a name and, under it, a fainter line (where
// the folder is, or a drive's free space) and, for a drive, a bar of how
// full it is. Hovered tiles lighten, the selected one fills. A click
// selects it, a double-click opens it, a right-click asks for its menu at
// the pointer (in the window).
Rectangle {
    id: tile

    required property string icon
    property color iconColor: Tokens.fg
    required property string title
    property string detail: ""
    // How full, 0 to 1, when it shows a bar (`measured`).
    property bool measured: false
    property real fill: 0
    property bool selected: false

    signal picked
    signal opened
    signal menuRequested(point at)

    width: Tokens.filesTileWidth
    height: Tokens.filesTileHeight
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
            size: Tokens.filesTileIconSize
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
            Rectangle {
                visible: tile.measured
                width: parent.width
                height: Tokens.filesDriveBarHeight
                radius: height / 2
                color: Tokens.track

                Rectangle {
                    width: parent.width * tile.fill
                    height: parent.height
                    radius: parent.radius
                    color: Tokens.meter

                    Behavior on width {
                        NumberAnimation {
                            duration: Tokens.durationFill
                            easing.type: Easing.BezierSpline
                            easing.bezierCurve: Tokens.easing
                        }
                    }
                }
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
