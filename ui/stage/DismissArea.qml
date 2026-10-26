import QtQuick
import Quickshell

// Every screen, on a stage surface (Stage, PanelSurface), while a panel
// that closes on a click outside is open: a click or scroll off what
// `stagePanels` covers, or Esc, asks to close it. A scroll closes it too, so the
// next one reaches the window below: the screen's input can't pass it on.
// It covers every screen, not just the surface: while a surface holds the
// keyboard, Hyprland gives it every click that isn't on such a surface, on
// any screen, at coordinates outside it (IMP-178). Other compositors clip
// the mask to the surface and send the rest to ClickCatcher.
MouseArea {
    id: area

    // What's drawn on this surface.
    required property StagePanels stagePanels
    // Its surface's screen.
    required property StageScreen output
    // Where its surface sits on that screen.
    property point origin: Qt.point(0, 0)
    // Square, as a mask shape (ShapeRegion) while the surface takes every
    // click.
    readonly property int radius: 0

    // What all the screens cover together, in the layout's coordinates.
    readonly property rect layout: {
        const screens = [area.output, ...Quickshell.screens.filter(screen => screen)];
        const left = Math.min(...screens.map(screen => screen.x));
        const top = Math.min(...screens.map(screen => screen.y));
        const right = Math.max(...screens.map(screen => screen.x + screen.width));
        const bottom = Math.max(...screens.map(screen => screen.y + screen.height));
        return Qt.rect(left, top, right - left, bottom - top);
    }
    x: layout.x - output.x - origin.x
    y: layout.y - output.y - origin.y
    width: layout.width
    height: layout.height

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
