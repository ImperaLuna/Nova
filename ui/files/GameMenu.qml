pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "games.js" as Games

// A game's right-click menu, at the pointer, over the whole window: Open
// install folder and Open save folder (Games.actions), the second dimmed
// when no save folder was found. A click beside it closes it.
Item {
    id: menu

    // The game's row (GamesModel's), while the menu is open.
    property var game: null

    // A folder to go to was chosen.
    signal chosen(string folder)

    function open(game: var, at: point) {
        menu.game = game;
        card.x = Math.min(at.x, width - card.width);
        card.y = Math.min(at.y, height - card.height);
    }

    function close() {
        game = null;
    }

    visible: game !== null

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: menu.close()
    }

    Rectangle {
        id: card
        width: Tokens.menuWidth
        height: rows.implicitHeight + 2 * Tokens.menuPad
        radius: Tokens.radiusCard
        color: Tokens.bg
        border.width: Tokens.dividerWidth
        border.color: Tokens.glassRim

        Column {
            id: rows
            x: Tokens.menuPad
            y: Tokens.menuPad
            width: parent.width - 2 * x

            Repeater {
                model: Games.actions(menu.game)

                Rectangle {
                    id: action

                    required property var modelData
                    readonly property bool available: modelData.folder !== ""

                    width: parent.width
                    height: Tokens.menuRowHeight
                    radius: Tokens.menuRowRadius
                    opacity: available ? 1 : Tokens.dimOpacity
                    color: hover.hovered && available ? Tokens.selected : "transparent"

                    Row {
                        anchors.left: parent.left
                        anchors.leftMargin: Tokens.rowInset
                        anchors.verticalCenter: parent.verticalCenter
                        spacing: Tokens.rowInset

                        LineIcon {
                            anchors.verticalCenter: parent.verticalCenter
                            path: action.modelData.icon
                            color: Tokens.filesFaint
                        }
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            font.family: Tokens.fontSans
                            font.pixelSize: Tokens.fontSize
                            color: Tokens.fg
                            text: action.modelData.label
                        }
                    }

                    HoverHandler {
                        id: hover
                    }
                    TapHandler {
                        enabled: action.available
                        onTapped: {
                            menu.chosen(action.modelData.folder);
                            menu.close();
                        }
                    }
                }
            }
        }
    }
}
