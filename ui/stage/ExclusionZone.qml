import QtQuick
import Quickshell
import Quickshell.Wayland

// The space the bar keeps free at the top of a screen, so windows sit below
// it. An empty surface of its own: the stage reserves nothing, so panels
// opening on it never move windows, and this only changes when the bar
// itself shows or hides.
PanelWindow { // qmllint disable uncreatable-type
    id: zone

    required property int size

    anchors {
        top: true
        left: true
        right: true
    }
    exclusiveZone: size
    WlrLayershell.namespace: "nova-exclusion-zone"
    // A layer surface needs some size; nothing is drawn and it takes no input.
    implicitHeight: 1
    color: "transparent"
    mask: Region {}

    // Qt redraws every window once whenever an animation starts anywhere in
    // the UI, so each panel toggle would hand the compositor a new frame of
    // this empty surface. It draws only its own changes instead: updates go
    // back on for one frame when it changes, and off once that frame is out.
    // Layer-shell state (the zone, the size) is only sent with a frame.
    function redraw() {
        updatesEnabled = true;
    }
    onExclusiveZoneChanged: redraw()
    onWidthChanged: redraw()
    onHeightChanged: redraw()
    onDevicePixelRatioChanged: redraw()

    Connections {
        target: zone.contentItem.Window.window
        function onFrameSwapped() {
            zone.updatesEnabled = false;
        }
    }
}
