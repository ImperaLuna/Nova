import QtQuick

// Whether a change made to a surface (its layer, size, regions, what it
// shows) still waits to reach the screen, for the surface's
// `updatesEnabled`: `pending` from request() until a frame polished after
// the latest request has been swapped. Quickshell sends a layer-shell
// surface's state when a frame is polished, and the compositor applies it
// with that frame's commit, so a surface that stopped drawing before then
// kept its old state: the stage stayed on Overlay over a fullscreen window
// (IMP-116).
// It waits for the swap, not just the polish, as a polished frame isn't
// always drawn: the render thread drops one it can't begin, and its own
// retry is dropped as well once updates are off. And request() asks for the
// frame itself: Qt's threaded render loop drops an update asked for while
// any window is polishing, which could leave a change in no frame at all.
QtObject {
    id: out

    // The window the changes go out with: a QQuickWindow, which QML has no
    // type name for (a surface's is its content's `Window.window`).
    required property var window

    readonly property bool pending: target !== done

    // The frames polished and swapped so far. Each polished frame is
    // swapped once, in order, unless it's dropped, and an older frame's
    // swap can come after a newer one is polished: counting tells them
    // apart. Never more swapped than polished.
    property int polished: 0
    property int swapped: 0
    // Which frame the latest request goes out with: the first one polished
    // after it, or `waiting` until there is one, or `done` once it's
    // swapped.
    readonly property int waiting: -1
    readonly property int done: -2
    property int target: done

    // Still pending this long after a request: the frame never came or
    // was dropped, and nothing is drawing any more. Counting starts over
    // and the frame is asked for again. Several frames long, so it never
    // fires while frames are coming.
    readonly property int retryMs: 250

    // A change was made that goes out with the next frame.
    function request() {
        target = waiting;
        retry.restart();
        window?.requestUpdate();
    }

    readonly property Connections frames: Connections {
        target: out.window
        function onAfterAnimating() {
            out.polished += 1;
            if (out.target === out.waiting)
                out.target = out.polished;
        }
        function onFrameSwapped() {
            out.swapped = Math.min(out.swapped + 1, out.polished);
            if (out.target > 0 && out.swapped >= out.target)
                out.target = out.done;
        }
    }

    readonly property Timer retry: Timer {
        interval: out.retryMs
        repeat: true
        running: out.pending
        onTriggered: {
            out.swapped = out.polished;
            out.request();
        }
    }
}
