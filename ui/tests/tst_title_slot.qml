import QtQuick
import QtTest
import "../media"
import "../theme"

// The now-playing pill's title slot: a long title is kept to the slot's width
// and clipped there, so it can't draw over the artist after it.
Item {
    id: stage
    width: 600
    height: 100

    TestCase {
        id: test
        name: "TitleSlot"
        when: windowShown

        Component {
            id: slotComponent
            TitleSlot {}
        }

        function init() {
            Tokens.apply({
                bg: "#1D1D1D",
                fg: "#F5E7C6",
                fgDim: "#9A9A9A",
                primary: "#FD7014",
                onAccent: "#1D1D1D"
            });
        }

        function test_a_long_title_is_kept_to_the_slot_and_clipped() {
            const slot = createTemporaryObject(slotComponent, stage, {
                text: "A title far too long to fit in the pill's slot at all, so it must scroll"
            });
            verify(slot !== null);
            verify(slot.overflow > 0);
            compare(slot.width, Tokens.mediaTitleWidth);
            compare(slot.clip, true);
        }

        function test_a_short_title_takes_only_its_own_width() {
            const slot = createTemporaryObject(slotComponent, stage, {
                text: "Hi"
            });
            verify(slot !== null);
            compare(slot.overflow, 0);
            verify(slot.width < Tokens.mediaTitleWidth);
        }
    }
}
