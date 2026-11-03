import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// A row of a Nova Files menu: its icon and label, filled while hovered,
// and a check at the end when it's the option that's on (`checked`).
Rectangle {
    id: row

    required property string icon
    required property string label
    property bool checked: false

    signal chosen

    width: parent?.width ?? 0
    height: Tokens.menuRowHeight
    radius: Tokens.menuRowRadius
    color: hover.hovered ? Tokens.selected : "transparent"

    Row {
        anchors.left: parent.left
        anchors.leftMargin: Tokens.rowInset
        anchors.verticalCenter: parent.verticalCenter
        spacing: Tokens.rowInset

        LineIcon {
            anchors.verticalCenter: parent.verticalCenter
            path: row.icon
            color: Tokens.filesFaint
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            color: Tokens.fg
            text: row.label
        }
    }
    LineIcon {
        anchors.right: parent.right
        anchors.rightMargin: Tokens.rowInset
        anchors.verticalCenter: parent.verticalCenter
        visible: row.checked
        path: Icons.CHECK
    }

    HoverHandler {
        id: hover
    }
    TapHandler {
        onTapped: row.chosen()
    }
}
