import QtQuick

// Whether a change made to a surface (its layer, size, regions, what it
// shows) still waits to reach the screen, for the surface's
// `updatesEnabled`: `pending` from request() until a frame polished after
// the latest request has been presented. Quickshell sends a layer-shell
// surface's state when a frame is polished, and the compositor applies it
// with that frame's commit, so a surface that stopped drawing before then
// kept its old state: the stage stayed on Overlay over a fullscreen window
// (IMP-116).
// A polished frame isn't always presented: the render thread drops one it
// can't begin (and its own retry is dropped too once updates are off), and
// the basic render loop draws one nothing asked to update without
// presenting it. Such a frame is asked for again. And request() asks for
// the frame itself: the threaded render loop drops an update asked for
// while any window is polishing, which could leave a change in no frame.
QtObject {
    id: out

    // The window the changes go out with: a QQuickWindow, which QML has no
    // type name for (a surface's is its content's `Window.window`).
    required property var window

    readonly property bool pending: target !== done

    // The frames polished and ended (presented or not) so far. Each
    // polished frame ends once, in order, and an older frame can end after
    // a newer one is polished: counting tells them apart. Never more ended
    // than polished.
    property int polished: 0
    property int ended: 0
    // Whether the frame ending now was presented.
    property bool presented: false
    // Which frame the latest request goes out with: the first one polished
    // after it, or `waiting` until there is one, or `done` once it's
    // presented.
    readonly property int waiting: -1
    readonly property int done: -2
    property int target: done

    // No frame at all this long after a request (an update dropped, or a
    // frame lost without ending): counting starts over and the frame is
    // asked for again, a while later each time, up to retryMaxMs.
    readonly property int retryMs: 250
    readonly property int retryMaxMs: 4000

    // A change was made that goes out with the next frame.
    function request() {
        target = waiting;
        retry.interval = retryMs;
        retry.restart();
        askForFrame();
    }

    // update() so the basic loop presents the frame, requestUpdate() as
    // the threaded loop can drop update()'s request.
    function askForFrame() {
        window?.update();
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
            out.presented = true;
        }
        function onAfterFrameEnd() {
            const presented = out.presented;
            out.presented = false;
            out.ended = Math.min(out.ended + 1, out.polished);
            if (out.target <= 0 || out.ended < out.target)
                return;
            if (presented) {
                out.target = out.done;
                return;
            }
            out.target = out.waiting;
            out.askForFrame();
        }
    }

    readonly property Timer retry: Timer {
        running: out.pending
        onTriggered: {
            out.ended = out.polished;
            out.target = out.waiting;
            out.retry.interval = Math.min(out.retry.interval * 2, out.retryMaxMs);
            out.retry.restart();
            out.askForFrame();
        }
    }
}
