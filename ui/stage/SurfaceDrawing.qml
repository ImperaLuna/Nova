import QtQuick
import Quickshell

// When a stage surface (Stage, PanelSurface) draws, for its
// `updatesEnabled`: while something moves on it, or while nothing moves on
// any surface (StageMotion), and until a frame with its last change is out
// (`settling`, FrameOut). Its regions only change when its own items move,
// and Quickshell sends them only with a frame, so drawing until that frame
// keeps them current.
QtObject {
    id: drawing

    required property QsWindow window
    // "<screen>/<placement>", as StageMotion lists it.
    required property string name
    required property bool moving
    // Its items just stopped, or something else changed (settle()), and
    // that isn't on screen yet.
    readonly property bool settling: out.pending
    readonly property bool draws: moving || settling || StageMotion.surfaces.length === 0

    // Draws until a frame with what just changed is out: for changes that
    // go out with a frame, like the surface's layer, size or regions.
    function settle() {
        out.request();
    }

    onMovingChanged: {
        StageMotion.setMoving(name, moving);
        if (!moving)
            settle();
    }
    Component.onDestruction: StageMotion.setMoving(name, false)

    readonly property FrameOut out: FrameOut {
        window: drawing.window.contentItem.Window.window
    }
}
