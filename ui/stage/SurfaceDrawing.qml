import QtQuick
import Quickshell

// When a stage surface (Stage, PanelSurface) draws, for its
// `updatesEnabled`: while something moves on it, or while nothing moves on
// any surface (StageMotion), and until its last frame is out (`settling`).
// Its regions only change when its own items move, and Quickshell sends
// them only with a frame, so drawing until that frame keeps them current.
QtObject {
    id: drawing

    required property QsWindow window
    // "<screen>/<placement>", as StageMotion lists it.
    required property string name
    required property bool moving
    // Its items just stopped (or something else changed) and that isn't in
    // a frame yet. Set by hand for other changes that go out with a frame.
    property bool settling: false
    readonly property bool draws: moving || settling || StageMotion.surfaces.length === 0

    onMovingChanged: {
        StageMotion.setMoving(name, moving);
        if (!moving)
            settling = true;
    }
    Component.onDestruction: StageMotion.setMoving(name, false)

    // Qt emits afterAnimating once it has polished a frame (Quickshell sends
    // the regions then) and right before syncing it, and that frame goes out
    // even if updates go off now. A later frameSwapped could still be an
    // older frame's.
    readonly property Connections settled: Connections {
        target: drawing.window.contentItem.Window.window
        enabled: drawing.settling
        function onAfterAnimating() {
            drawing.settling = false;
        }
    }
}
