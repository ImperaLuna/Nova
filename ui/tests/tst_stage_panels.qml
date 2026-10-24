import QtQuick
import QtTest
import "../stage"

// What the stage works out from the bar's pills (where it takes clicks,
// where it blurs, when it holds the keyboard, closing what's open), and how
// opening one panel closes the ones it rules out, on any screen.
Item {
    id: root

    Rectangle {
        id: fixedPill
        radius: 18
    }

    StagePanel {
        id: usage
        name: "ai-usage"
    }

    StagePanel {
        id: launcher
        name: "launcher"
        glass: false
        wantsKeyboard: true
    }

    StagePanel {
        id: notes
        name: "notes"
        exclusive: false
    }

    // The same panel on another screen.
    StagePanel {
        id: usageElsewhere
        name: "ai-usage"
    }

    StagePanel {
        id: notesElsewhere
        name: "notes"
        exclusive: false
    }

    StagePanels {
        id: drawn
        panels: [usage, launcher, notes]
        fixed: [fixedPill]
    }

    TestCase {
        name: "StagePanels"

        function cleanup() {
            for (const panel of [usage, launcher, notes, usageElsewhere, notesElsewhere])
                panel.open = false;
            drawn.panels = [usage, launcher, notes];
        }

        function test_every_drawn_item_takes_clicks() {
            compare(drawn.shapes.length, 4);
            for (const item of [fixedPill, usage, launcher, notes])
                verify(drawn.shapes.includes(item));
        }

        function test_only_glass_panels_are_blurred() {
            compare(drawn.glassShapes.length, 2);
            verify(!drawn.glassShapes.includes(launcher));
            verify(!drawn.glassShapes.includes(fixedPill));
        }

        function test_keyboard_is_held_only_while_a_typing_panel_is_open() {
            verify(!drawn.needsKeyboard);
            usage.open = true;
            verify(!drawn.needsKeyboard, "an open panel that takes no typing");
            launcher.open = true;
            verify(drawn.needsKeyboard);
        }

        function test_dismiss_closes_the_panels_holding_the_keyboard() {
            notes.open = true;
            launcher.open = true;
            drawn.dismiss();
            verify(!launcher.open);
            verify(!drawn.needsKeyboard);
            verify(notes.open, "notes stays");
        }

        function test_opening_an_exclusive_panel_closes_the_other_exclusive_ones() {
            usage.open = true;
            notes.open = true;
            launcher.open = true;
            verify(!usage.open);
            verify(launcher.open);
            verify(notes.open, "notes isn't exclusive");
            usage.open = true;
            verify(!launcher.open);
        }

        function test_a_panel_opens_on_one_screen_at_a_time() {
            usage.open = true;
            usageElsewhere.open = true;
            verify(!usage.open);
            notes.open = true;
            notesElsewhere.open = true;
            verify(!notes.open, "even a panel that isn't exclusive");
        }

        function test_toggle_acts_only_on_the_focused_screen() {
            drawn.toggle("ai-usage", "DP-1", "HDMI-A-1");
            verify(!usage.open, "another screen is focused");
            drawn.toggle("ai-usage", "DP-1", "DP-1");
            verify(usage.open);
            verify(!launcher.open && !notes.open, "only the named panel");
            drawn.toggle("ai-usage", "DP-1", "DP-1");
            verify(!usage.open, "the same toggle closes it");
        }

        function test_toggle_moves_a_panel_to_the_focused_screen() {
            usageElsewhere.open = true;
            drawn.toggle("ai-usage", "DP-1", "DP-1");
            verify(usage.open);
            verify(!usageElsewhere.open);
        }

        function test_panels_can_come_and_go() {
            drawn.panels = [usage];
            compare(drawn.shapes.length, 2);
            compare(drawn.glassShapes.length, 1);
            drawn.panels = [];
            compare(drawn.shapes.length, 1);
            compare(drawn.glassShapes.length, 0);
        }
    }
}
