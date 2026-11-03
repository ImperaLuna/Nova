import QtQuick
import "../theme"

// A Nova Files menu over the whole window: a card of MenuRows (its
// children) placed at a point by `place`, shown while `shown`. A click
// beside it or Escape closes it (`dismissed`). It holds the keyboard while
// shown, so keys meant for it never reach what's under it, and says when
// it's gone (`hidden`) for the window to take the keyboard back.
Item {
    id: menu

    property bool shown: false
    default property alias rows: rows.data

    signal dismissed
    signal hidden

    // Puts the card's top left at `at`, kept inside the window.
    function place(at: point) {
        card.x = Math.max(0, Math.min(at.x, width - card.width));
        card.y = Math.max(0, Math.min(at.y, height - card.height));
    }

    visible: shown

    onShownChanged: {
        if (shown)
            forceActiveFocus();
        else
            hidden();
    }
    Keys.onPressed: event => {
        if (event.key === Qt.Key_Escape)
            menu.dismissed();
    }

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
