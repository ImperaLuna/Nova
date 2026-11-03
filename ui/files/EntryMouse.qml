import QtQuick
import "../theme"

// The mouse on an entry of Nova Files' list, a Details row or an Icons
// tile alike: a left press is `entryPressed`, and once it moves far enough
// it asks to drag (`dragRequested`); one that doesn't move is a click
// (`entryClicked`). A right press asks for the menu at the pointer
// (`menuAsked`), a middle click is its own (`middleClicked`) and a double
// click opens the entry (`entryOpened`). It hovers, so the entry can
// lighten, and keeps the press from the list's scrolling while dragging.
MouseArea {
    id: mouse

    signal entryPressed(int modifiers)
    // Released where it was pressed, without dragging.
    signal entryClicked(int modifiers)
    signal dragRequested
    signal entryOpened
    signal middleClicked
    // A right-click at `at` (in the entry's parent, which this fills).
    signal menuAsked(point at)

    hoverEnabled: true
    acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton
    // A drag takes the entries along; the wheel scrolls the list.
    preventStealing: true

    // Where the press was, until it moved far enough to drag.
    property point pressedAt
    property bool dragging: false

    onPressed: press => {
        if (press.button === Qt.RightButton) {
            mouse.menuAsked(Qt.point(press.x, press.y));
            return;
        }
        pressedAt = Qt.point(press.x, press.y);
        dragging = false;
        if (press.button === Qt.LeftButton)
            mouse.entryPressed(press.modifiers);
    }
    onPositionChanged: move => {
        if ((pressedButtons & Qt.LeftButton) && !dragging && Math.hypot(move.x - pressedAt.x, move.y - pressedAt.y) >= Tokens.filesDragStart) {
            dragging = true;
            mouse.dragRequested();
        }
    }
    onClicked: click => {
        if (click.button === Qt.MiddleButton)
            mouse.middleClicked();
        else if (click.button === Qt.LeftButton && !dragging)
            mouse.entryClicked(click.modifiers);
    }
    onDoubleClicked: press => {
        if (press.button === Qt.LeftButton)
            mouse.entryOpened();
    }
}
