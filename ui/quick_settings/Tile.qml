import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// A toggle tile: an icon and a title, filled with the accent while on. The
// fill says its state, so no line under the title does. With `opens`, an
// arrow at its end opens the tile's page instead of toggling.
Item {
    id: tile

    required property string icon
    // The icon's solid parts (LineIcon `fill`).
    property string iconFill: ""
    required property string title
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

    Text {
        anchors.verticalCenter: parent.verticalCenter
        x: glyph.x + glyph.width + Tokens.rowInset
        width: (tile.opens ? arrow.x : tile.width - Tokens.pad) - x
        elide: Text.ElideRight
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSize
        font.weight: Tokens.weightTitle
        color: tile.ink
        text: tile.title
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
