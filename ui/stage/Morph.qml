import QtQuick
import "../theme"

// The "grow from this item" motion every panel uses. `t` runs from 0 (the
// item it grows from, e.g. a pill) to 1 (the open panel) on the theme's
// panel duration and curve; sizes, positions and radii in between come from
// lerp(). Content inside fades in after the panel has grown with MorphReveal.
QtObject {
    id: morph

    property bool open: false
    // 0 closed, 1 open.
    property real t: open ? 1 : 0
    // Whether it's growing or shrinking right now.
    readonly property bool moving: motion.running

    // A value between its closed (`from`) and open (`to`) form, for now.
    function lerp(from: real, to: real): real {
        return from + (to - from) * t;
    }

    Behavior on t {
        NumberAnimation {
            id: motion
            duration: Tokens.durationPanel
            easing.type: Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
    }
}
