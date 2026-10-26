import QtQuick

// The whole screen, on a stage surface (Stage, PanelSurface), while a panel
// that closes on a click outside is open: a click or scroll off what
// `stagePanels` covers, or Esc, asks to close it. A scroll closes it too, so the
// next one reaches the window below: the screen's input can't pass it on.
// It covers the whole screen, not just the surface, since Hyprland gives a
// surface that holds the keyboard every click on the screen, at coordinates
// outside it; other compositors clip the mask to the surface and send the
// rest to ClickCatcher. Its owner places it over the screen.
MouseArea {
    id: area

    // What's drawn on this surface.
    required property StagePanels stagePanels
    // Square, as a mask shape (ShapeRegion) while the surface takes every
    // click.
    readonly property int radius: 0

    signal dismissRequested

    focus: true
    onClicked: mouse => dismissOutside(mouse.x, mouse.y)
    onWheel: wheel => dismissOutside(wheel.x, wheel.y)
    Keys.onEscapePressed: dismissRequested()

    function dismissOutside(x: real, y: real) {
        const at = mapToItem(null, x, y);
        if (!stagePanels.covers(at.x, at.y))
            dismissRequested();
    }
}
