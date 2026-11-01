import QtQuick
import QtTest
import "../central_pill"
import "../theme"

// The real central pill, fed workspaces in the shape Protocol.barWorkspaces
// gives it. The root is an Item: a TestCase root gets no mouse events.
Item {
    id: stage
    width: 600
    height: 100

    TestCase {
        id: test
        name: "CentralPill"
        when: windowShown

        Component {
            id: pillComponent
            CentralPill {}
        }

        SignalSpy {
            id: focusSpy
            signalName: "focusRequested"
        }

        SignalSpy {
            id: stepSpy
            signalName: "stepRequested"
        }

        Component {
            id: signalSpy
            SignalSpy {}
        }

        function workspace(id, active, urgent) {
            return {
                id: id,
                name: String(id),
                active: active ?? false,
                focused: false,
                urgent: urgent ?? false,
                empty: false
            };
        }

        function init() {
            Tokens.apply({
                bg: "#1D1D1D",
                fg: "#F5E7C6",
                primary: "#FD7014",
                onAccent: "#1D1D1D"
            });
        }

        function createPill(workspaces) {
            const pill = createTemporaryObject(pillComponent, stage, {
                workspaces: workspaces,
                outputFocused: true,
                canFocus: true,
                now: new Date(2026, 9, 24, 16, 40)
            });
            pill.width = Qt.binding(() => pill.implicitWidth);
            pill.height = Qt.binding(() => pill.implicitHeight);
            focusSpy.target = pill;
            focusSpy.clear();
            stepSpy.target = pill;
            stepSpy.clear();
            return pill;
        }

        // The number cells, in order.
        function numbers(pill) {
            const list = findChild(pill, "numbers").children.filter(child => child.workspaceId !== undefined);
            return list.sort((a, b) => a.index - b.index);
        }

        function settle(pill) {
            tryVerify(() => Math.abs(pill.width - pill.implicitWidth) < 0.01 && pill.numbersRoom === (pill.workspaces.length ? pill.numbersWidth + pill.numbersFrame : 0));
            wait(Tokens.durationPanel);
        }

        function test_it_lists_the_workspaces_in_order() {
            const pill = createPill([workspace(1, true), workspace(2), workspace(5)]);
            settle(pill);
            compare(numbers(pill).map(number => number.name), ["1", "2", "5"]);
            compare(pill.height, Tokens.barHeight);
        }

        function test_the_pill_grows_left_and_the_time_stays() {
            const pill = createPill([workspace(1, true), workspace(2)]);
            settle(pill);
            const width = pill.width;
            const fromRight = pill.timeFromRight;
            const first = numbers(pill)[0];
            pill.workspaces = [workspace(1, true), workspace(2), workspace(3)];
            settle(pill);
            compare(pill.width, width + Tokens.workspaceSize + Tokens.workspaceGap);
            compare(pill.timeFromRight, fromRight);
            verify(numbers(pill)[0] === first, "the numbers already there stay");
            pill.workspaces = [workspace(1, true), workspace(3)];
            settle(pill);
            compare(pill.width, width);
            compare(numbers(pill).map(number => number.name), ["1", "3"]);
        }

        function test_the_search_button_sits_at_the_left_end_and_asks_for_the_launcher() {
            for (const workspaces of [[workspace(1, true), workspace(2)], []]) {
                const pill = createPill(workspaces);
                settle(pill);
                const width = pill.width;
                const fromRight = pill.timeFromRight;
                const search = findChild(pill, "search");
                verify(!search.visible);
                pill.canSearch = true;
                settle(pill);
                verify(search.visible);
                verify(pill.width > width, "it grows to the left");
                compare(pill.timeFromRight, fromRight, "the time stays");
                for (const number of numbers(pill))
                    verify(number.mapToItem(pill, 0, 0).x > search.x + search.width, "the numbers come after it");
                const searched = signalSpy.createObject(pill, {
                    target: pill,
                    signalName: "searchRequested"
                });
                mouseClick(search, search.width / 2, search.height / 2);
                compare(searched.count, 1);
            }
        }

        function test_the_capsule_slides_to_the_shown_workspace() {
            const pill = createPill([workspace(1, true), workspace(2), workspace(3)]);
            settle(pill);
            const capsule = findChild(pill, "capsule");
            compare(capsule.x, numbers(pill)[0].x);
            verify(Qt.colorEqual(capsule.color, Tokens.primary));
            pill.workspaces = [workspace(1), workspace(2), workspace(3, true)];
            verify(capsule.x < numbers(pill)[2].x, "it slides rather than jumps");
            tryCompare(capsule, "x", numbers(pill)[2].x);
            pill.outputFocused = false;
            tryVerify(() => Qt.colorEqual(capsule.color, Tokens.selectedQuiet));
        }

        function test_clicking_a_number_asks_to_go_there() {
            const pill = createPill([workspace(1, true), workspace(7)]);
            settle(pill);
            const seven = numbers(pill)[1];
            mouseClick(seven, seven.width / 2, seven.height / 2);
            compare(focusSpy.count, 1);
            compare(focusSpy.signalArguments[0][0], 7);
            pill.canFocus = false;
            mouseClick(seven, seven.width / 2, seven.height / 2);
            compare(focusSpy.count, 1, "not where the compositor can't");
        }

        function test_scrolling_steps_one_workspace_per_notch() {
            const pill = createPill([workspace(1, true), workspace(2)]);
            settle(pill);
            mouseWheel(pill, pill.width / 2, pill.height / 2, 0, -120);
            compare(stepSpy.count, 1);
            compare(stepSpy.signalArguments[0][0], "next");
            // A touchpad's small steps add up to one.
            for (let i = 0; i < 3; i++)
                mouseWheel(pill, pill.width / 2, pill.height / 2, 0, 40);
            compare(stepSpy.count, 2);
            compare(stepSpy.signalArguments[1][0], "previous");
        }

        function test_without_workspaces_it_is_the_day_and_time() {
            const pill = createPill([]);
            settle(pill);
            compare(numbers(pill).length, 0);
            compare(findChild(pill, "divider").opacity, 0);
            const listed = createPill([workspace(1, true)]);
            settle(listed);
            verify(pill.width < listed.width);
            compare(pill.timeFromRight, listed.timeFromRight);
        }

        function test_only_the_changing_digits_roll() {
            const pill = createPill([]);
            pill.now = new Date(2026, 9, 24, 16, 59);
            wait(Tokens.durationPanel * 2);
            const time = findChild(pill, "time");
            compare(time.text, "16:59");
            pill.now = new Date(2026, 9, 24, 17, 0);
            compare(time.text, "17:00");
            const digits = time.children.filter(slot => slot.isDigit).map(slot => slot.children[0]);
            compare(digits.map(digit => digit.shown), ["1", "7", "0", "0"]);
            compare(digits[0].roll, 1, "1 stays put");
            verify(digits[1].roll < 1, "6 rolls to 7");
            tryCompare(digits[1], "roll", 1);
        }

        function special(active, elsewhere) {
            return Object.assign(workspace(-98, active), {
                name: "magic",
                special: true,
                shown_elsewhere: elsewhere ?? false
            });
        }

        function test_a_special_workspace_leads_a_little_apart() {
            const pill = createPill([workspace(1, true), workspace(2)]);
            settle(pill);
            const width = pill.width;
            pill.workspaces = [special(), workspace(1, true), workspace(2)];
            settle(pill);
            compare(pill.width, width + Tokens.workspaceSize + Tokens.workspaceGap + Tokens.workspaceGroupGap);
            const [mark, one] = numbers(pill);
            verify(mark.special && mark.mark, "the only one: a tilde");
            compare(one.x - mark.x, pill.step + Tokens.workspaceGroupGap);
            verify(Qt.colorEqual(mark.ink, Tokens.fgDim), "dim while hidden");
            pill.workspaces = [special(false, true), workspace(1, true), workspace(2)];
            tryVerify(() => Qt.colorEqual(mark.ink, Tokens.fg), 1000, "brighter while it shows on another output");
        }

        function test_the_capsule_moves_onto_a_shown_special_workspace() {
            const pill = createPill([special(), workspace(1, true), workspace(2)]);
            settle(pill);
            const capsule = findChild(pill, "capsule");
            compare(capsule.x, numbers(pill)[1].x);
            pill.workspaces = [special(true), workspace(1), workspace(2)];
            tryCompare(capsule, "x", numbers(pill)[0].x);
            verify(Qt.colorEqual(numbers(pill)[0].ink, Tokens.onAccent));
            verify(Qt.colorEqual(numbers(pill)[1].ink, Tokens.fg), "the workspace under it isn't lit");
            const mark = numbers(pill)[0];
            mouseClick(mark, mark.width / 2, mark.height / 2);
            compare(focusSpy.count, 1);
            compare(focusSpy.signalArguments[0][0], -98, "clicking it hides it again");
        }

        function test_several_special_workspaces_show_their_letters() {
            const music = Object.assign(special(), {
                id: -99,
                name: "music"
            });
            const pill = createPill([special(), music, workspace(1, true)]);
            settle(pill);
            const [magic, second] = numbers(pill);
            verify(!magic.mark && !second.mark);
            compare(findChild(magic, "label").text, "M");
            compare(findChild(numbers(pill)[2], "label").text, "1");
        }

        function test_a_special_workspace_never_calls() {
            const pill = createPill([Object.assign(special(), {
                    urgent: true
                }), workspace(1, true)]);
            settle(pill);
            verify(!numbers(pill)[0].calling);
        }

        function test_an_urgent_workspace_is_marked_until_shown() {
            const pill = createPill([workspace(1, true), workspace(2, false, true)]);
            settle(pill);
            verify(numbers(pill)[1].calling);
            pill.workspaces = [workspace(1), workspace(2, true, true)];
            verify(!numbers(pill)[1].calling);
        }

        // A still dot: it fades in once and then holds, so a workspace left
        // urgent costs no frames while it waits.
        function test_the_urgent_dot_holds_still() {
            const pill = createPill([workspace(1, true), workspace(2, false, true)]);
            settle(pill);
            const dot = findChild(numbers(pill)[1], "dot");
            tryCompare(dot, "opacity", 1);
            wait(Tokens.durationFade * 8);
            compare(dot.opacity, 1);
            pill.workspaces = [workspace(1), workspace(2, true, true)];
            tryCompare(dot, "visible", false);
        }
    }
}
