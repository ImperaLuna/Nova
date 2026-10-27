import QtQuick
import Quickshell
import Quickshell.Wayland
import "../theme"

// The wallpaper carousel's preview on a screen it isn't open on: the
// screen's pick over the whole screen ("" while it keeps its own), and
// PickingFor while the carousel picks for it; taking no clicks or keyboard,
// fading in and out with `shown`. On the
// overlay layer, like the carousel, so it covers fullscreen windows too.
PanelWindow { // qmllint disable uncreatable-type
    id: window

    required property string path
    property bool shown: false
    property bool picking: false

    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    mask: Region {}

    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "nova-wallpaper-preview"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    PreviewImage {
        anchors.fill: parent
        path: window.path
        pixels: Qt.size(window.width * Screen.devicePixelRatio, window.height * Screen.devicePixelRatio)
        opacity: window.shown ? 1 : 0
        Behavior on opacity {
            NumberAnimation {
                duration: Tokens.durationPanel
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }
    }

    PickingFor {
        anchors.fill: parent
        shown: window.shown && window.picking
    }
}
