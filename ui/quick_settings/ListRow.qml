import QtQuick
import "../theme"

// A list row: an icon, a name with a line under it, and a status at its
// end ("In use"), quietly filled while selected.
Item {
    id: row

    required property string icon
    required property string title
    property string subtitle: ""
    property string status: ""
    property bool selected: false

    signal clicked

    implicitHeight: Tokens.powerRowHeight

    Rectangle {
        anchors.fill: parent
        radius: Tokens.radiusRow
        color: press.containsMouse ? Tokens.hover : row.selected ? Tokens.surfaceRaised : "transparent"
    }

    MouseArea {
        id: press
        anchors.fill: parent
        hoverEnabled: true
        onClicked: row.clicked()
    }

    LineIcon {
        id: glyph
        anchors.verticalCenter: parent.verticalCenter
        x: Tokens.pad
        path: row.icon
        size: Tokens.iconSizeMenu
    }

    Column {
        anchors.verticalCenter: parent.verticalCenter
        x: glyph.x + glyph.width + Tokens.pad
        width: statusText.x - x - Tokens.gap

        Text {
            width: parent.width
            elide: Text.ElideRight
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            font.weight: Font.Medium
            color: Tokens.fg
            textFormat: Text.PlainText
            text: row.title
        }
        Text {
            visible: text !== ""
            width: parent.width
            elide: Text.ElideRight
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeTiny
            color: Tokens.fgDim
            textFormat: Text.PlainText
            text: row.subtitle
        }
    }

    Text {
        id: statusText
        anchors.verticalCenter: parent.verticalCenter
        anchors.right: parent.right
        anchors.rightMargin: Tokens.pad
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeTiny
        color: row.selected ? Tokens.primary : Tokens.fgDim
        text: row.status
    }
}
