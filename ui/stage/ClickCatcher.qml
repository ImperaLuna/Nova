pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland

// While a panel that closes on a click outside is open (`catching`), the
// rest of the screen: a click or scroll anywhere off Nova's surfaces closes
// that panel. Those surfaces take clicks over their whole area meanwhile, so the
// two never overlap and it doesn't matter which the compositor puts on top.
// Empty, and never blurred. It stays mapped, taking no clicks while not
// catching: Quickshell builds a layer-shell window anew each time it shows,
// with a render thread and graphics context of its own, which held up every
// animation on the screen as the panel opened (IMP-72).
PanelWindow { // qmllint disable uncreatable-type
    id: catcher

    // Whether it takes clicks.
    required property bool catching
    // The surfaces' areas on the screen, each a rect, cut out of it.
    required property list<rect> holes
    // The stage's layer (Stage `surfaceLayer`): over a fullscreen window while
    // catching, and never left over one once it's done.
    required property int surfaceLayer

    signal clicked

    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: catcher.surfaceLayer
    // Its own name, so a compositor rule meant for Nova's glass (blur) never
    // matches this full-screen surface.
    WlrLayershell.namespace: "nova-click-catcher"

    mask: Region {
        item: catcher.catching ? area : null
        regions: cutouts.instances
    }

    Variants {
        id: cutouts
        model: catcher.holes
        Region {
            required property rect modelData
            x: modelData.x
            y: modelData.y
            width: modelData.width
            height: modelData.height
            intersection: Intersection.Subtract
        }
    }

    // It draws nothing, so it would only hand the compositor a new empty
    // frame for every animation frame anywhere. It draws its own changes
    // (its mask, layer and layer-shell size go out with a frame) until a
    // frame with them is out (FrameOut): Hyprland cuts the mask down to the
    // size of the last frame, so one from before a resize would leave it
    // taking no clicks.
    updatesEnabled: out.pending
    function redraw() {
        out.request();
    }
    onVisibleChanged: redraw()
    onCatchingChanged: redraw()
    onHolesChanged: redraw()
    onSurfaceLayerChanged: redraw()
    onWidthChanged: redraw()
    onHeightChanged: redraw()
    onDevicePixelRatioChanged: redraw()

    FrameOut {
        id: out
        window: catcher.contentItem.Window.window
    }

    MouseArea {
        id: area
        anchors.fill: parent
        onClicked: catcher.clicked()
        // A scroll closes the panel too, so the next one reaches the window
        // below: it can't pass this one on.
        onWheel: catcher.clicked()
    }
}
