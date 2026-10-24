import QtQuick
import QtTest
import "../stage"
import "../theme"

// The shared grow-from-a-pill motion: the morph itself, and content that
// fades in once it has grown and leaves with it.
Item {
    id: root

    Morph {
        id: morph
    }

    Rectangle {
        id: content
        opacity: morph.open ? 1 : 0
        MorphReveal on opacity {
            id: reveal
            delay: 0.6
        }
    }

    TestCase {
        name: "Morph"
        when: windowShown

        function cleanup() {
            morph.open = false;
            // Until it has settled: tryCompare's fuzzy match on t passes a
            // frame just short of the end.
            tryVerify(() => !morph.moving && !reveal.running);
            compare(morph.t, 0);
            compare(content.opacity, 0);
        }

        function test_t_runs_from_0_to_1_and_back() {
            compare(morph.t, 0);
            morph.open = true;
            tryCompare(morph, "t", 1, Tokens.durationPanel * 4);
            morph.open = false;
            tryCompare(morph, "t", 0, Tokens.durationPanel * 4);
        }

        function test_it_moves_only_while_it_grows_or_shrinks() {
            verify(!morph.moving);
            morph.open = true;
            verify(morph.moving);
            tryVerify(() => !morph.moving, Tokens.durationPanel * 4);
            compare(morph.t, 1);
        }

        function test_reveal_runs_through_its_delay_and_fade() {
            morph.open = true;
            verify(reveal.running);
            tryVerify(() => !reveal.running, Tokens.durationPanel * 4);
            compare(content.opacity, 1);
        }

        function test_lerp_goes_from_the_closed_value_to_the_open_one() {
            compare(morph.lerp(36, 300), 36);
            morph.open = true;
            // Exactly 1: tryCompare's fuzzy match also passes a frame just
            // short of the end, which lerp scales past compare's tolerance.
            tryVerify(() => morph.t === 1);
            compare(morph.lerp(36, 300), 300);
        }

        function test_content_waits_for_the_panel_to_grow() {
            morph.open = true;
            // Still inside its delay: the panel is growing, the content waits.
            wait(Tokens.durationPanel * 0.3);
            compare(content.opacity, 0);
            verify(morph.t > 0);
            tryCompare(content, "opacity", 1, Tokens.durationPanel * 4);
        }

        function test_content_leaves_with_the_panel() {
            morph.open = true;
            tryCompare(content, "opacity", 1, Tokens.durationPanel * 4);
            morph.open = false;
            // No delay on the way out: it starts fading at once.
            wait(Tokens.durationPanel * 0.5);
            verify(content.opacity < 1, `opacity ${content.opacity}`);
            tryCompare(content, "opacity", 0, Tokens.durationPanel * 4);
        }
    }
}
