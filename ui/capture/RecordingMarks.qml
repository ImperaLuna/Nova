pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import "../stage"
import "../theme"

// The corner marks around a region being recorded, on its screen: just
// outside it, so they aren't in the recording. Their own surface on the
// overlay layer, taking no clicks, keyboard or space.
PanelWindow { // qmllint disable uncreatable-type
    id: window

    screen: output.screen
    // Its screen (StageScreen), kept while it goes away.
    required property StageScreen output
    // The region, in layout pixels.
    required property var region

    // The region on this screen, and the box the marks sit on.
    readonly property real _left: region.x - output.x - Tokens.recordMarkGap
    readonly property real _top: region.y - output.y - Tokens.recordMarkGap
    readonly property real _width: region.width + 2 * Tokens.recordMarkGap
    readonly property real _height: region.height + 2 * Tokens.recordMarkGap

    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "nova-recording"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    mask: Region {}

    // Each corner: which way its arms go from it.
    Repeater {
        model: [
            {
                right: false,
                down: false
            },
            {
                right: true,
                down: false
            },
            {
                right: false,
                down: true
            },
            {
                right: true,
                down: true
            }
        ]

        Item {
            id: corner
            required property var modelData
            readonly property real cornerX: modelData.right ? window._left + window._width : window._left
            readonly property real cornerY: modelData.down ? window._top + window._height : window._top

            Rectangle {
                x: corner.modelData.right ? corner.cornerX - Tokens.recordMarkLength : corner.cornerX - Tokens.recordMarkWidth
                y: corner.modelData.down ? corner.cornerY : corner.cornerY - Tokens.recordMarkWidth
                width: Tokens.recordMarkLength + Tokens.recordMarkWidth
                height: Tokens.recordMarkWidth
                color: Tokens.error
            }
            Rectangle {
                x: corner.modelData.right ? corner.cornerX : corner.cornerX - Tokens.recordMarkWidth
                y: corner.modelData.down ? corner.cornerY - Tokens.recordMarkLength : corner.cornerY - Tokens.recordMarkWidth
                width: Tokens.recordMarkWidth
                height: Tokens.recordMarkLength + Tokens.recordMarkWidth
                color: Tokens.error
            }
        }
    }
}
