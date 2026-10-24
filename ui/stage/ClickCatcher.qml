pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland

// While a panel holds the keyboard, the rest of the screen: a click anywhere
// off Nova's surfaces closes that panel. Those surfaces take clicks over
// their whole area meanwhile, so the two never overlap and it doesn't matter
// which the compositor puts on top. Empty, and never blurred.
PanelWindow { // qmllint disable uncreatable-type
    id: catcher

    // The surfaces' areas on the screen, each a rect, cut out of it.
    required property list<rect> holes

    signal clicked

    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Top

    mask: Region {
        item: area
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

    // Like ExclusionZone: it draws nothing, so it would only hand the
    // compositor a new empty frame for every animation frame anywhere. It
    // draws its own changes (its mask and layer-shell size go out with a
    // frame) and stops once that frame is out.
    function redraw() {
        updatesEnabled = true;
    }
    onVisibleChanged: redraw()
    onHolesChanged: redraw()
    onWidthChanged: redraw()
    onHeightChanged: redraw()
    onDevicePixelRatioChanged: redraw()

    Connections {
        target: catcher.contentItem.Window.window
        function onFrameSwapped() {
            catcher.updatesEnabled = false;
        }
    }

    MouseArea {
        id: area
        anchors.fill: parent
        onClicked: catcher.clicked()
    }
}
