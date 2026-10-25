import QtQuick
import "../theme"

// A pinned app: its icon on a raised tile, its name under it on hover.
// Clicking it launches the app.
Item {
    id: tile

    required property string appId
    required property string name
    property string iconSource: ""

    signal clicked

    implicitWidth: Tokens.launcherTileSize
    implicitHeight: Tokens.launcherTileSize + Tokens.launcherTileName

    Rectangle {
        id: face
        width: Tokens.launcherTileSize
        height: Tokens.launcherTileSize
        radius: Tokens.radiusCard
        color: Tokens.surfaceRaised

        Rectangle {
            anchors.fill: parent
            radius: parent.radius
            color: Tokens.hover
            opacity: mouse.containsMouse ? 1 : 0
            Behavior on opacity {
                NumberAnimation {
                    duration: Tokens.durationFast
                }
            }
        }

        AppIcon {
            anchors.centerIn: parent
            appId: tile.appId
            name: tile.name
            source: tile.iconSource
            size: Tokens.launcherTileIcon
        }
    }

    Text {
        anchors.horizontalCenter: face.horizontalCenter
        anchors.top: face.bottom
        width: Math.min(implicitWidth, Tokens.launcherTileSize + 2 * Tokens.launcherGap)
        height: Tokens.launcherTileName
        verticalAlignment: Text.AlignBottom
        horizontalAlignment: Text.AlignHCenter
        elide: Text.ElideRight
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeTiny
        color: Tokens.fgDim
        textFormat: Text.PlainText
        text: tile.name
        opacity: mouse.containsMouse ? 1 : 0
        Behavior on opacity {
            NumberAnimation {
                duration: Tokens.durationFast
            }
        }
    }

    MouseArea {
        id: mouse
        anchors.fill: face
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: tile.clicked()
    }
}
