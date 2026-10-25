pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import "../theme"

// One per screen and placement off the stage ("right", "center"): the layer
// surface its panels are drawn on. It's there only while one of them is open
// or still closing, and only as big as what they draw (their `area`, plus
// shadow): niri blurs the whole surface behind glass on every frame it
// draws (IMP-54). It keeps that size while they move, since a new size
// waits a round trip to the compositor. It reserves no space, so windows
// never move, and takes input only where something is drawn.
//
// The panels' parent becomes its content, which covers the whole screen
// however small the surface is, so each panel places itself in screen
// coordinates. The same StagePanel flags work here as on the stage.
PanelWindow { // qmllint disable uncreatable-type
    id: surface

    screen: output
    // Its screen, typed (the window's own `screen` isn't, to qmllint).
    required property ShellScreen output
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
    // Whether anything on it is animating.
    readonly property bool moving: drawn.moving
    // Its panels just stopped and their final state isn't in a frame yet.
    property bool settling: false
    onMovingChanged: {
        StageMotion.setMoving(motionName, moving);
        if (!moving)
            settling = true;
    }
    Component.onDestruction: StageMotion.setMoving(motionName, false)
    readonly property string motionName: `${output.name}/${placement}`

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
    function boxAround(area: rect): rect {
        const top = Math.max(0, Math.floor(area.y - Tokens.shadowRoom));
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

    // Shown while panels are, and on until the item a closing panel grew out
    // of shows on the stage again. Set by hand rather than bound, so it
    // never flickers off and on while those change together.
    visible: false
    readonly property bool hasShown: drawn.shown.length > 0
    onHasShownChanged: {
        if (hasShown) {
            visible = true;
            return;
        }
        const handingBack = panels.some(panel => panel.covering);
        for (const panel of panels)
            panel.covering = false;
        // Clearing `covering` makes the stage redraw its item, and hold.
        if (!handingBack || !holding)
            visible = false;
    }
    onHoldingChanged: {
        if (!holding && !hasShown)
            visible = false;
    }

    anchors {
        top: true
        right: placement === "right"
    }
    margins.top: box.y
    implicitWidth: Math.max(1, box.width)
    implicitHeight: Math.max(1, box.height)
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    // Like the stage (StageMotion): draws while it moves, or while nothing
    // moves anywhere, and until its last frame is out.
    updatesEnabled: drawn.moving || settling || StageMotion.surfaces.length === 0

    Connections {
        target: surface.contentItem.Window.window
        function onAfterAnimating() {
            surface.settling = false;
        }
        // Once a frame with a panel drawn over its `from` is out, `from` can
        // hide.
        function onFrameSwapped() {
            for (const panel of surface.drawn.shown) {
                if (panel.from && !panel.covering)
                    panel.covering = true;
            }
        }
    }

    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.namespace: "nova-panel"
    WlrLayershell.keyboardFocus: drawn.needsKeyboard ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    mask: Region {
        regions: inputRegions.instances
    }

    Variants {
        id: inputRegions
        model: surface.clickAnywhere ? [outside] : surface.drawn.shapes
        ShapeRegion {}
    }

    Region {
        id: blur
        regions: blurRegions.instances
    }

    Variants {
        id: blurRegions
        model: surface.drawn.glassShapes
        ShapeRegion {}
    }

    // As on the stage: asked for only once there's glass.
    readonly property bool hasGlass: drawn.glassShapes.length > 0
    function askForBlur() {
        if (hasGlass)
            BackgroundEffect.blurRegion = blur;
    }
    onHasGlassChanged: askForBlur()
    Component.onCompleted: {
        askForBlur();
        adopt();
    }

    // The whole screen, not just the surface: Hyprland gives a surface that
    // holds the keyboard every click on the screen, at coordinates outside
    // it. Other compositors clip the mask to the surface and send the rest
    // to ClickCatcher.
    MouseArea {
        id: outside
        readonly property int radius: 0
        x: -surface.box.x
        y: -surface.box.y
        width: surface.output.width
        height: surface.output.height
        enabled: surface.clickAnywhere
        focus: true
        onClicked: mouse => {
            const at = mapToItem(null, mouse.x, mouse.y);
            if (!surface.drawn.covers(at.x, at.y))
                surface.dismissRequested();
        }
        // As on the stage: a scroll outside closes it too.
        onWheel: wheel => {
            const at = mapToItem(null, wheel.x, wheel.y);
            if (!surface.drawn.covers(at.x, at.y))
                surface.dismissRequested();
        }
        Keys.onEscapePressed: surface.dismissRequested()
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
        surfaceName: surface.motionName
    }
}
