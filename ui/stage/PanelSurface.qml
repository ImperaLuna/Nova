pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import "../theme"

// One per screen and placement off the stage ("right", "center"): the layer
// surface its panels are drawn on. While one of them is open or still
// closing it's as big as what they draw (their `area`, plus shadow): niri
// blurs the whole surface behind glass on every frame it draws (IMP-54). It
// keeps that size while they move, since a new size waits a round trip to
// the compositor. Otherwise it's parked: 1x1, taking no clicks, no blur. It
// stays mapped because Quickshell builds a layer-shell window anew each
// time it shows, with a render thread and graphics context of its own,
// which held back a panel's first frame by ~50 ms (IMP-148, as ClickCatcher
// found in IMP-72). It reserves no space, so windows
// never move, and takes input only where something is drawn.
//
// The panels' parent becomes its content, which covers the whole screen
// however small the surface is, so each panel places itself in screen
// coordinates. The same StagePanel flags work here as on the stage.
PanelWindow { // qmllint disable uncreatable-type
    id: surface

    screen: output.screen
    // Its screen, as the stage lays out by it (StageScreen).
    required property StageScreen output
    // "right" (against the right edge) or "center" (centered across).
    required property string placement
    // This placement's panels on this screen.
    required property list<StagePanel> panels
    // A panel that closes on a click outside is open on this screen: the
    // whole surface takes clicks, and one off its panels closes that panel.
    required property bool clickAnywhere
    // The stage hasn't yet drawn the item a closing panel grew out of
    // (StagePanel `from`) back: the surface stays until it has.
    property bool holding: false

    // Esc, or a click off the panels while `clickAnywhere`.
    signal dismissRequested

    readonly property StagePanels drawn: StagePanels {
        panels: surface.panels
    }
    // When it draws, like the stage: while its panels move, or nothing
    // moves anywhere, and until their last frame is out.
    readonly property SurfaceDrawing drawing: SurfaceDrawing {
        window: surface
        name: `${surface.output.name}/${surface.placement}`
        moving: surface.drawn.moving
    }

    // Where it sits on the screen: the shown panels' room plus shadow,
    // within the screen. A centered surface spreads evenly either side of
    // the middle, since the compositor centers it. It keeps its last size
    // once nothing is shown, so it doesn't change on its way out.
    property rect box: Qt.rect(0, 0, 0, 0)
    readonly property rect room: drawn.room
    onRoomChanged: {
        if (room.width > 0 && room.height > 0)
            box = boxAround(room);
    }
    // A panel below the bar's row (a tray menu) keeps its surface off the
    // row: there the surface would take the clicks meant for the pills
    // under it, and its shadow room covered them on niri.
    function boxAround(area: rect): rect {
        const barRow = Tokens.barGap + Tokens.barHeight;
        const top = Math.max(area.y >= barRow ? barRow : 0, Math.floor(area.y - Tokens.shadowRoom));
        const bottom = Math.min(output.height, Math.ceil(area.y + area.height + Tokens.shadowRoom));
        let left = Math.max(0, Math.floor(area.x - Tokens.shadowRoom));
        if (placement === "center") {
            const middle = output.width / 2;
            const half = Math.min(middle, Math.max(middle - left, Math.ceil(area.x + area.width + Tokens.shadowRoom) - middle));
            left = Math.floor(middle - half);
            return Qt.rect(left, top, Math.ceil(2 * half), bottom - top);
        }
        return Qt.rect(left, top, output.width - left, bottom - top);
    }

    // Parked unless panels are shown, and until the item a closing panel grew
    // out of shows on the stage again. Set by hand rather than bound, so it
    // never flickers while those change together.
    property bool parked: true
    // Parks it, and draws once so the 1x1 size and empty regions go out:
    // Quickshell sends them only with a frame.
    function park() {
        if (parked)
            return;
        parked = true;
        drawing.settle();
    }
    readonly property bool hasShown: drawn.shown.length > 0
    onHasShownChanged: {
        if (hasShown) {
            parked = false;
            return;
        }
        const handingBack = panels.some(panel => panel.covering);
        for (const panel of panels)
            panel.covering = false;
        // Clearing `covering` makes the stage redraw its item, and hold.
        if (!handingBack || !holding)
            park();
    }
    // A panel that closed while another stays open here (quick settings
    // giving way to the power menu) hands its item back now: the surface
    // isn't going away, so that can't wait for it to.
    Connections {
        target: surface.drawn
        function onShownChanged() {
            if (!surface.hasShown)
                return;
            for (const panel of surface.panels) {
                if (panel.covering && !surface.drawn.shown.includes(panel))
                    panel.covering = false;
            }
        }
    }
    onHoldingChanged: {
        if (!holding && !hasShown)
            park();
    }

    anchors {
        top: true
        right: placement === "right"
    }
    margins.top: parked ? 0 : box.y
    implicitWidth: parked ? 1 : Math.max(1, box.width)
    implicitHeight: parked ? 1 : Math.max(1, box.height)
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    // Parked, only its parking frame (and the first, which maps it): it has
    // nothing to draw, and would otherwise hand the compositor an empty
    // frame each time animations elsewhere stop.
    updatesEnabled: drawing.settling || (!parked && drawing.draws)

    // Once a frame with a panel drawn over its `from` is out, `from` can
    // hide.
    Connections {
        target: surface.contentItem.Window.window
        function onFrameSwapped() {
            for (const panel of surface.drawn.shown) {
                if (panel.from && !panel.covering)
                    panel.covering = true;
            }
        }
    }

    // Always over a fullscreen window (Stage `surfaceLayer`), even parked:
    // it never changes layer, which Hyprland ignored before a first frame.
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "nova-panel"
    WlrLayershell.keyboardFocus: drawn.needsKeyboard ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    mask: regions.mask
    StageRegions {
        id: regions
        window: surface
        inputShapes: surface.parked ? [] : surface.clickAnywhere ? [outside] : surface.drawn.shapes
        glassShapes: surface.parked ? [] : surface.drawn.glassShapes
    }
    Component.onCompleted: {
        adopt();
        drawing.settle();
    }

    // Over every screen (DismissArea).
    DismissArea {
        id: outside
        output: surface.output
        origin: Qt.point(surface.box.x, surface.box.y)
        enabled: surface.clickAnywhere
        stagePanels: surface.drawn
        onDismissRequested: surface.dismissRequested()
    }

    // The screen, placed so that what's on it lines up with the surface.
    Item {
        id: content
        x: -surface.box.x
        y: -surface.box.y
        width: surface.output.width
        height: surface.output.height
    }

    function adopt() {
        for (const panel of panels)
            panel.parent = content;
    }
    onPanelsChanged: adopt()

    FrameLog {
        surfaceName: surface.drawing.name
    }
}
