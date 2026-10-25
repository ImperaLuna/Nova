pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import "../capture"
import "../capture/capture.js" as Capture
import "../daemon/protocol.js" as Protocol

// The region picker's sums (capture.js), the picker taking a drag, a click
// or a screen, and the screenshot card's buttons and drain, without novad
// or a window: those need Quickshell, which the VM tests cover. The frame
// is 400x300 on a 200x150 picker: a screen at scale 2. The root is an Item:
// a TestCase root gets no mouse events.
Item {
    id: stage
    width: 400
    height: 600

    TestCase {
        id: test
        name: "Capture"
        when: windowShown

        readonly property string framePath: Qt.resolvedUrl("data/frame-400x300.png").toString().replace("file://", "")

        Component {
            id: pickerComponent
            RegionPicker {
                width: 200
                height: 150
                framePath: ""
                mode: Protocol.CAPTURE_REGION
            }
        }

        Component {
            id: cardComponent
            ShotCard {
                shot: ({
                        id: 7,
                        path: test.framePath,
                        width: 960,
                        height: 560,
                        copied: true
                    })
            }
        }

        SignalSpy {
            id: saves
            signalName: "saveRequested"
        }

        SignalSpy {
            id: cancels
            signalName: "cancelRequested"
        }

        function makePicker(mode) {
            const picker = createTemporaryObject(pickerComponent, stage, {
                framePath: framePath,
                mode: mode ?? Protocol.CAPTURE_REGION
            });
            saves.clear();
            cancels.clear();
            saves.target = picker;
            cancels.target = picker;
            tryCompare(picker, "frameScale", 2);
            return picker;
        }

        function test_a_drag_is_a_region_either_way() {
            compare(Capture.region(10, 20, 40, 60), {
                x: 10,
                y: 20,
                width: 30,
                height: 40
            });
            compare(Capture.region(40, 60, 10, 20), Capture.region(10, 20, 40, 60));
            verify(Capture.isClick(Capture.region(5, 5, 7, 40), 4));
            verify(!Capture.isClick(Capture.region(5, 5, 9, 9), 4));
        }

        function test_regions_become_whole_frame_pixels_on_the_frame() {
            // Fractional scale: rounded outward.
            compare(Capture.toPixels({
                x: 10.3,
                y: 0,
                width: 20,
                height: 10
            }, 1.5, 400, 300), {
                x: 15,
                y: 0,
                width: 31,
                height: 15
            });
            // Past the edge: trimmed to the frame.
            compare(Capture.toPixels({
                x: 190,
                y: 140,
                width: 50,
                height: 50
            }, 2, 400, 300), {
                x: 380,
                y: 280,
                width: 20,
                height: 20
            });
        }

        function test_frames_and_labels() {
            const frames = [
                {
                    output: "DP-1",
                    path: "/run/a.png"
                },
                {
                    output: "HDMI-A-1",
                    path: "/run/b.png"
                }
            ];
            compare(Capture.frameFor(frames, "HDMI-A-1").path, "/run/b.png");
            compare(Capture.frameFor(frames, "eDP-1"), null);
            compare(Capture.frameFor(null, "DP-1"), null);
            compare(Capture.size(960, 560), "960 × 560");
            compare(Capture.folderLabel("/home/me/Pictures/Screenshots/a.png"), "Pictures/Screenshots");
            compare(Capture.fileUrl("/home/me/shots#1/a b?.png"), "file:///home/me/shots%231/a%20b%3F.png");
        }

        function test_letting_go_of_a_drag_takes_the_region_in_frame_pixels() {
            const picker = makePicker();
            mousePress(picker, 20, 60);
            mouseMove(picker, 60, 90);
            verify(picker.dragging);
            mouseRelease(picker, 60, 90);
            compare(saves.count, 1);
            compare(saves.signalArguments[0][0], {
                x: 40,
                y: 120,
                width: 80,
                height: 60
            });
        }

        function test_a_click_takes_nothing_and_the_picker_stays() {
            const picker = makePicker();
            mouseClick(picker, 100, 100);
            compare(saves.count, 0);
            compare(picker.region, null);
            compare(cancels.count, 0);
        }

        function test_screen_mode_takes_the_whole_frame_on_a_click() {
            const picker = makePicker(Protocol.CAPTURE_SCREEN);
            mouseClick(picker, 100, 100);
            compare(saves.count, 1);
            compare(saves.signalArguments[0][0], {
                x: 0,
                y: 0,
                width: 400,
                height: 300
            });
        }

        function test_esc_and_a_right_click_close_it() {
            const picker = makePicker();
            verify(picker.key(Qt.Key_Escape));
            verify(!picker.key(Qt.Key_A));
            mouseClick(picker, 100, 100, Qt.RightButton);
            compare(cancels.count, 2);
            compare(saves.count, 0);
        }

        function test_a_busy_picker_takes_nothing() {
            const picker = makePicker();
            picker.busy = true;
            mousePress(picker, 20, 60);
            mouseMove(picker, 60, 90);
            mouseRelease(picker, 60, 90);
            compare(saves.count, 0);
        }

        SignalSpy {
            id: actions
            signalName: "actionRequested"
        }

        SignalSpy {
            id: expiries
            signalName: "expired"
        }

        function test_the_card_asks_for_its_buttons_and_drains_away() {
            const card = createTemporaryObject(cardComponent, stage);
            actions.target = card;
            expiries.target = card;
            actions.clear();
            expiries.clear();
            verify(card.remaining > 0);
            const buttons = findButtons(card);
            compare(buttons.length, 3);
            mouseClick(buttons[2]);
            compare(actions.count, 1);
            compare(actions.signalArguments[0][0], Protocol.SHOT_DELETE);
            // The pointer on the card holds it; off it, it drains away.
            tryVerify(() => card.held);
            mouseMove(stage, stage.width - 1, stage.height - 1);
            verify(!card.held);
            tryCompare(expiries, "count", 1, 10000);
            compare(card.remaining, 0);
        }

        function findButtons(item) {
            let found = [];
            for (const child of item.children) {
                if (child instanceof CaptureButton)
                    found.push(child);
                found = found.concat(findButtons(child));
            }
            return found;
        }
    }
}
