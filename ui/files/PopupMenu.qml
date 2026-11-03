import QtQuick
import "../theme"

// A Nova Files menu over the whole window: a card of MenuRows (its
// children) placed at a point by `place`, shown while `shown`. A click
// beside it closes it (`dismissed`).
Item {
    id: menu

    property bool shown: false
    default property alias rows: rows.data

    signal dismissed

    // Puts the card's top left at `at`, kept inside the window.
    function place(at: point) {
        card.x = Math.max(0, Math.min(at.x, width - card.width));
        card.y = Math.max(0, Math.min(at.y, height - card.height));
    }

    visible: shown

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: menu.dismissed()
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
        }
    }
}
