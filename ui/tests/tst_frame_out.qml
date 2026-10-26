import QtQuick
import QtTest
import "../stage"

// When a change to a surface counts as on screen (FrameOut): once a frame
// polished after it has been swapped, not before, even when an older
// frame's swap comes late or the frame is dropped (IMP-116).
Item {
    id: root

    // Never shown, so it draws nothing by itself: the tests play its
    // frames by emitting the window's signals.
    Window {
        id: still
        visible: false
    }

    FrameOut {
        id: out
        window: still
    }

    // A window that draws, for a request to go out on its own.
    Window {
        id: shown
        width: 40
        height: 40
        visible: true
        Rectangle {
            anchors.fill: parent
            color: "red"
        }
    }

    FrameOut {
        id: real
        window: shown
    }

    // One frame of `still`: polished, then swapped.
    function frame() {
        still.afterAnimating();
        still.frameSwapped();
    }

    TestCase {
        name: "FrameOut"
        when: windowShown

        function init() {
            root.frame();
            verify(!out.pending);
        }

        function test_a_request_is_pending_until_a_frame_after_it_is_swapped() {
            out.request();
            verify(out.pending);
            still.afterAnimating();
            verify(out.pending);
            still.frameSwapped();
            verify(!out.pending);
        }

        function test_an_older_frames_late_swap_is_not_the_one() {
            // Polished before the request, swapped after it.
            still.afterAnimating();
            out.request();
            still.frameSwapped();
            verify(out.pending);
            // The next frame, with the change, is still being drawn.
            still.afterAnimating();
            verify(out.pending);
            still.frameSwapped();
            verify(!out.pending);
        }

        function test_a_new_request_waits_for_a_newer_frame() {
            out.request();
            still.afterAnimating();
            // Another change, while the first's frame is drawn.
            out.request();
            still.frameSwapped();
            verify(out.pending);
            root.frame();
            verify(!out.pending);
        }

        function test_a_dropped_frame_is_asked_for_again() {
            out.request();
            // Polished, but never drawn.
            still.afterAnimating();
            wait(out.retryMs * 2);
            verify(out.pending);
            // Counting started over: the next frame is the one.
            compare(out.target, out.waiting);
            root.frame();
            verify(!out.pending);
        }

        function test_a_drawing_window_gets_its_change_out() {
            tryVerify(() => !real.pending);
            shown.color = "blue";
            real.request();
            verify(real.pending);
            tryVerify(() => !real.pending, 2000);
        }
    }
}
