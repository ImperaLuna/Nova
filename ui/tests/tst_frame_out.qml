import QtQuick
import QtTest
import "../stage"

// When a change to a surface counts as on screen (FrameOut): once a frame
// polished after it has been presented, not before, even when an older
// frame ends late or a frame is dropped (IMP-116).
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

    // One frame of `still`: polished, then presented, then ended.
    function frame() {
        still.afterAnimating();
        root.end(true);
    }

    // The end of `still`'s oldest frame still being drawn.
    function end(presented: bool) {
        if (presented)
            still.frameSwapped();
        still.afterFrameEnd();
    }

    TestCase {
        name: "FrameOut"
        when: windowShown

        function init() {
            root.frame();
            verify(!out.pending);
        }

        function test_a_request_is_pending_until_a_frame_after_it_is_presented() {
            out.request();
            verify(out.pending);
            still.afterAnimating();
            verify(out.pending);
            root.end(true);
            verify(!out.pending);
        }

        function test_an_older_frame_ending_late_is_not_the_one() {
            // Polished before the request, ended after it.
            still.afterAnimating();
            out.request();
            root.end(true);
            verify(out.pending);
            // The next frame, with the change, is still being drawn.
            still.afterAnimating();
            verify(out.pending);
            root.end(true);
            verify(!out.pending);
        }

        function test_a_new_request_waits_for_a_newer_frame() {
            out.request();
            still.afterAnimating();
            // Another change, while the first's frame is drawn.
            out.request();
            root.end(true);
            verify(out.pending);
            root.frame();
            verify(!out.pending);
        }

        function test_a_frame_that_is_not_presented_is_asked_for_again() {
            out.request();
            still.afterAnimating();
            root.end(false);
            verify(out.pending);
            compare(out.target, out.waiting);
            root.frame();
            verify(!out.pending);
        }

        function test_with_no_frame_at_all_it_asks_again() {
            out.request();
            // Polished, but it never ends.
            still.afterAnimating();
            wait(out.retryMs * 1.5);
            verify(out.pending);
            // Counting started over: the next frame is the one.
            compare(out.target, out.waiting);
            compare(out.retry.interval, out.retryMs * 2);
            root.frame();
            verify(!out.pending);
        }

        function test_a_shown_window_gets_a_change_out_that_draws_nothing_new() {
            tryVerify(() => !real.pending);
            // Like a new layer or mask: nothing in the scene changes.
            real.request();
            verify(real.pending);
            const presented = real.polished;
            tryVerify(() => !real.pending, 2000);
            verify(real.polished > presented);
        }
    }
}
