import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// A toggle tile: an icon, a title and a line under it, filled with the
// accent while on. With `opens`, an arrow at its end opens the tile's page
// instead of toggling.
Item {
    id: tile

    required property string icon
    // The icon's solid parts (LineIcon `fill`).
    property string iconFill: ""
    required property string title
    required property string subtitle
    property bool on: false
    property bool available: true
    property bool opens: false

    signal toggled
    signal opened

    readonly property color ink: on ? Tokens.onAccent : Tokens.fg

    implicitHeight: Tokens.tileHeight
    opacity: available ? 1 : Tokens.dimOpacity

    Rectangle {
        anchors.fill: parent
        radius: Tokens.radiusCard
        color: tile.on ? Tokens.primary : body.containsMouse ? Tokens.hover : Tokens.surfaceRaised

        Behavior on color {
            ColorAnimation {
                duration: Tokens.durationColor
            }
        }
    }

    MouseArea {
        id: body
        anchors.fill: parent
        hoverEnabled: true
        enabled: tile.available
        onClicked: tile.toggled()
    }

    LineIcon {
        id: glyph
        anchors.verticalCenter: parent.verticalCenter
        x: Tokens.pad + Tokens.labelNudge
        path: tile.icon
        fill: tile.iconFill
        size: Tokens.iconSizeMenu
        color: tile.ink
    }

    Column {
        anchors.verticalCenter: parent.verticalCenter
        x: glyph.x + glyph.width + Tokens.rowInset
        width: (tile.opens ? arrow.x : tile.width - Tokens.pad) - x

        Text {
            width: parent.width
            elide: Text.ElideRight
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            font.weight: Font.DemiBold
            color: tile.ink
            text: tile.title
        }

        Text {
            width: parent.width
            elide: Text.ElideRight
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeTiny
            color: tile.on ? Tokens.alpha(tile.ink, Tokens.tileSubOpacity) : Tokens.fgDim
            textFormat: Text.PlainText
            text: tile.subtitle
        }
    }

    Item {
        id: arrow
        visible: tile.opens
        anchors.right: parent.right
        anchors.rightMargin: Tokens.labelNudge * 2
        width: Tokens.tileArrowWidth + Tokens.gap
        height: parent.height

        LineIcon {
            anchors.centerIn: parent
            path: Icons.RIGHT
            color: tile.ink
        }

        MouseArea {
            anchors.fill: parent
            enabled: tile.available
            onClicked: tile.opened()
        }
    }
}
