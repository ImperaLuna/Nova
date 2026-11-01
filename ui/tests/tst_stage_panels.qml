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
        closesOnClickOutside: false
    }

    // A part drawn beside a panel, like the power tab's fillets: a square
    // with the circle centred on its top-left corner cut out.
    Item {
        id: fillet
        readonly property int radius: 0
        readonly property Item cutout: Item {
            parent: fillet
            x: -fillet.width
            y: -fillet.height
            width: 2 * fillet.width
            height: 2 * fillet.height
        }
        x: 500
        y: 100
        width: 20
        height: 20
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
        closesOnClickOutside: false
    }

    StagePanels {
        id: drawn
        panels: [usage, launcher, notes]
        fixed: [fixedPill]
    }

    TestCase {
        name: "StagePanels"

        function cleanup() {
            for (const panel of [usage, launcher, notes, usageElsewhere, notesElsewhere]) {
                panel.open = false;
                panel.moving = false;
            }
            usage.x = usage.y = usage.width = usage.height = 0;
            fixedPill.x = fixedPill.width = 0;
            fixedPill.visible = true;
            notes.x = notes.width = notes.height = 0;
            usage.parts = [];
            fillet.visible = true;
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

        function test_reach_makes_room_for_panels_before_they_open() {
            fixedPill.y = 8;
            fixedPill.height = 36;
            compare(drawn.reach, 44, "closed panels are no taller than the pill");
            usage.y = 8;
            usage.openHeight = 400;
            compare(drawn.reach, 408, "a closed panel already counts at its open height");
            launcher.openHeight = 600;
            compare(drawn.reach, 600);
        }

        function test_a_hidden_item_takes_no_clicks_but_keeps_its_room() {
            fixedPill.y = 8;
            fixedPill.height = 36;
            fixedPill.visible = false;
            verify(!drawn.shapes.includes(fixedPill), "a pill a panel elsewhere covers");
            compare(drawn.reach, 44, "the stage doesn't shrink under it");
            fixedPill.visible = true;
            verify(drawn.shapes.includes(fixedPill));
        }

        // The AI usage panel open over the now-playing pill beside it: that
        // pill hides while the panel is open over it.
        function test_an_item_an_open_panel_is_over_hides() {
            fixedPill.x = 316;
            fixedPill.y = 8;
            fixedPill.width = 200;
            fixedPill.height = 36;
            usage.x = 16;
            usage.y = 8;
            usage.width = 290;
            usage.height = 36;
            usage.open = true;
            compare(drawn.underPanels.length, 0, "beside it, a gap apart");
            usage.width = 301;
            verify(drawn.underPanels.includes(fixedPill), "grown over its edge");
            verify(!drawn.underPanels.includes(usage), "never under itself");
            fixedPill.x = 400;
            verify(!drawn.underPanels.includes(fixedPill), "moved clear of it");
            fixedPill.x = 300;
            verify(drawn.underPanels.includes(fixedPill), "moved under it");
            fixedPill.visible = false;
            verify(drawn.underPanels.includes(fixedPill), "still under it once hidden");
            usage.open = false;
            usage.moving = true;
            compare(drawn.underPanels.length, 0, "closing, it's drawn below again");
        }

        // A closed pill whose content moves (a scrolling title) isn't drawn
        // over the open panel beside it, and open panels never hide each
        // other.
        function test_only_an_open_panel_hides_what_it_is_over() {
            usage.x = 16;
            usage.width = 420;
            usage.height = 300;
            usage.open = true;
            notes.x = 300;
            notes.width = 200;
            notes.height = 36;
            notes.moving = true;
            verify(drawn.underPanels.includes(notes), "a closed pill under it");
            verify(!drawn.underPanels.includes(usage), "its content moving doesn't hide the open one");
            notes.moving = false;
            notes.open = true;
            compare(drawn.underPanels.length, 0, "both open");
        }

        function test_a_click_on_a_drawn_item_is_not_outside() {
            usage.x = 100;
            usage.y = 8;
            usage.width = 300;
            usage.height = 400;
            verify(drawn.covers(250, 200), "inside the panel, where nothing in it takes the click");
            verify(!drawn.covers(50, 200));
            verify(!drawn.covers(250, 500));
        }

        function test_a_panels_parts_take_clicks_and_blur_but_not_their_cutout() {
            usage.parts = [fillet];
            verify(drawn.shapes.includes(fillet));
            verify(drawn.glassShapes.includes(fillet), "glass like its panel");
            verify(drawn.covers(518, 118), "the corner the cut leaves");
            verify(!drawn.covers(503, 103), "inside the cut circle");
            verify(!drawn.covers(530, 110));
            fillet.visible = false;
            verify(!drawn.shapes.includes(fillet), "hidden, it takes nothing");
            verify(!drawn.covers(518, 118));
        }

        function test_a_part_of_a_panel_that_isnt_glass_isnt_blurred() {
            launcher.parts = [fillet];
            verify(drawn.shapes.includes(fillet));
            verify(!drawn.glassShapes.includes(fillet));
            launcher.parts = [];
        }

        function test_room_covers_the_shown_panels_areas() {
            usage.area = Qt.rect(100, 50, 300, 900);
            notes.area = Qt.rect(450, 400, 70, 300);
            compare(drawn.shown.length, 0);
            compare(drawn.room.width, 0, "nothing shown, no room");
            usage.open = true;
            compare(drawn.room, Qt.rect(100, 50, 300, 900));
            notes.open = true;
            compare(drawn.room, Qt.rect(100, 50, 420, 900), "both together");
            usage.open = false;
            usage.moving = true;
            compare(drawn.room, Qt.rect(100, 50, 420, 900), "still closing");
            usage.moving = false;
            compare(drawn.room, Qt.rect(450, 400, 70, 300));
        }

        function test_keyboard_is_held_while_a_panel_that_closes_on_esc_is_open() {
            verify(!drawn.needsKeyboard);
            notes.open = true;
            verify(!drawn.needsKeyboard, "notes stays open on Esc, so it doesn't hold the keyboard");
            usage.open = true;
            verify(drawn.needsKeyboard, "a panel that takes no typing still holds it for Esc");
            usage.open = false;
            launcher.open = true;
            verify(drawn.needsKeyboard);
        }

        function test_esc_closes_every_panel_but_one_that_opts_out() {
            verify(usage.closesOnClickOutside, "a panel does by default");
            notes.open = true;
            launcher.open = true;
            drawn.dismiss();
            verify(!launcher.open);
            verify(!drawn.needsKeyboard);
            verify(notes.open, "notes stays");
            usage.open = true;
            drawn.dismiss();
            verify(!usage.open);
            verify(notes.open, "notes stays");
        }

        function test_the_screen_takes_clicks_while_a_panel_closes_on_one_outside() {
            verify(!drawn.catchesClicks);
            notes.open = true;
            verify(!drawn.catchesClicks, "notes stays open on a click outside");
            usage.open = true;
            verify(drawn.catchesClicks);
            drawn.dismiss();
            verify(!usage.open);
            verify(notes.open, "notes stays");
            verify(!drawn.catchesClicks);
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

        function test_it_moves_while_any_panel_moves() {
            verify(!drawn.moving);
            notes.moving = true;
            verify(drawn.moving);
            notes.moving = false;
            verify(!drawn.moving);
            usageElsewhere.moving = true;
            verify(!drawn.moving, "a panel on another screen");
            usageElsewhere.moving = false;
        }

        function test_stage_motion_lists_each_moving_surface_once() {
            StageMotion.setMoving("DP-1/top", true);
            StageMotion.setMoving("DP-1/top", true);
            StageMotion.setMoving("DP-1/right", true);
            compare(StageMotion.surfaces.length, 2);
            StageMotion.setMoving("DP-1/top", false);
            compare(StageMotion.surfaces, ["DP-1/right"]);
            StageMotion.setMoving("DP-1/right", false);
            compare(StageMotion.surfaces.length, 0);
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
