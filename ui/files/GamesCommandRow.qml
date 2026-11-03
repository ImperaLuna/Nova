pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "games.js" as Games

// The command row while a window shows the games: Open install folder and
// Open save folder for the picked game (Games.actions), each dimmed while
// there's nowhere to go, then Sort and View.
Rectangle {
    id: row

    required property GamesModel model
    // How solid the window frame is (0..1): the row fades into it.
    property real solid: 0

    // A folder to go to was chosen.
    signal chosen(string folder)

    implicitHeight: actions.implicitHeight + Tokens.filesCommandPadTop + Tokens.gap
    color: Tokens.alpha(Tokens.surfaceRaised, Tokens.raisedOpacity * (1 - solid))

    component Separator: Rectangle {
        anchors.verticalCenter: parent?.verticalCenter
        width: Tokens.dividerWidth
        height: Tokens.filesSeparatorHeight
        color: Tokens.glassRim
    }

    Row {
        id: actions
        anchors.left: parent.left
        anchors.leftMargin: Tokens.rowInset
        anchors.top: parent.top
        anchors.topMargin: Tokens.filesCommandPadTop
        spacing: Tokens.labelNudge * 2

        Repeater {
            model: Games.actions(row.model.pickedRow)

            ToolButton {
                required property var modelData
                required property int index

                icon: modelData.icon
                label: modelData.label
                raised: index === 0 && enabled
                enabled: modelData.folder !== ""
                onClicked: row.chosen(modelData.folder)
            }
        }
        Separator {}
        ToolButton {
            icon: Icons.SORT
            label: qsTr("Sort")
            menu: true
            enabled: false
        }
        ToolButton {
            icon: Icons.VIEW
            label: qsTr("View")
            menu: true
            enabled: false
        }
    }
}
