import QtQuick
import QtTest
import "../stage"

// What the stage works out from the bar's pills: where it takes clicks,
// where it blurs, when it holds the keyboard, and closing what's open.
Item {
    id: root

    Rectangle {
        id: fixedPill
        radius: 18
    }

    StagePanel {
        id: glassPanel
    }

    StagePanel {
        id: typingPanel
        glass: false
        wantsKeyboard: true
    }

    StagePanels {
        id: drawn
        panels: [glassPanel, typingPanel]
        fixed: [fixedPill]
    }

    TestCase {
        name: "StagePanels"

        function cleanup() {
            glassPanel.open = false;
            typingPanel.open = false;
        }

        function test_every_drawn_item_takes_clicks() {
            compare(drawn.shapes.length, 3);
            verify(drawn.shapes.includes(fixedPill));
            verify(drawn.shapes.includes(glassPanel));
            verify(drawn.shapes.includes(typingPanel));
        }

        function test_only_glass_panels_are_blurred() {
            compare(drawn.glassShapes.length, 1);
            compare(drawn.glassShapes[0], glassPanel);
        }

        function test_keyboard_is_held_only_while_a_typing_panel_is_open() {
            verify(!drawn.needsKeyboard);
            glassPanel.open = true;
            verify(!drawn.needsKeyboard, "an open panel that takes no typing");
            typingPanel.open = true;
            verify(drawn.needsKeyboard);
        }

        function test_dismiss_closes_every_panel() {
            glassPanel.open = true;
            typingPanel.open = true;
            drawn.dismiss();
            verify(!glassPanel.open);
            verify(!typingPanel.open);
            verify(!drawn.needsKeyboard);
        }

        function test_panels_can_come_and_go() {
            drawn.panels = [glassPanel];
            compare(drawn.shapes.length, 2);
            compare(drawn.glassShapes.length, 1);
            drawn.panels = [];
            compare(drawn.shapes.length, 1);
            compare(drawn.glassShapes.length, 0);
            drawn.panels = [glassPanel, typingPanel];
        }
    }
}
