import QtQuick
import QtTest
import "../power"
import "../daemon/protocol.js" as Protocol

// The power menu's picking: starts on Lock, arrows skip what can't run,
// lock runs at once, shut down only on a second pick, Esc disarms before it
// closes, and nothing else runs while one action does.
TestCase {
    name: "PowerChoice"

    Component {
        id: choiceComponent
        PowerChoice {}
    }

    SignalSpy {
        id: runs
        signalName: "runRequested"
    }

    SignalSpy {
        id: closes
        signalName: "closeRequested"
    }

    function make(options) {
        const choice = createTemporaryObject(choiceComponent, null, {
            options: options ?? {}
        });
        runs.clear();
        closes.clear();
        runs.target = choice;
        closes.target = choice;
        return choice;
    }

    function test_lock_runs_at_once() {
        const choice = make();
        compare(choice.selected, Protocol.POWER_LOCK);
        verify(choice.key(Qt.Key_Return, "\r", false));
        compare(runs.count, 1);
        compare(runs.signalArguments[0][0], Protocol.POWER_LOCK);
        compare(choice.running, Protocol.POWER_LOCK);
    }

    function test_shut_down_needs_a_second_pick() {
        const choice = make();
        verify(choice.key(Qt.Key_P, "p", false));
        compare(choice.armed, Protocol.POWER_POWER_OFF);
        verify(choice.counting);
        compare(runs.count, 0);
        choice.pick(Protocol.POWER_POWER_OFF);
        compare(runs.count, 1);
        compare(choice.armed, "");
        verify(!choice.counting);
    }

    function test_esc_disarms_then_closes() {
        const choice = make();
        choice.pick(Protocol.POWER_REBOOT);
        choice.key(Qt.Key_Escape, "", false);
        compare(choice.armed, "");
        compare(closes.count, 0);
        choice.key(Qt.Key_Escape, "", false);
        compare(closes.count, 1);
    }

    function test_arrows_skip_what_cant_run() {
        const choice = make({
            [Protocol.POWER_SUSPEND]: {
                available: false,
                reason: "Turned off on this system"
            }
        });
        choice.key(Qt.Key_Down, "", false);
        compare(choice.selected, Protocol.POWER_LOG_OUT);
        choice.key(Qt.Key_Up, "", false);
        compare(choice.selected, Protocol.POWER_LOCK);
        choice.key(Qt.Key_Up, "", false);
        compare(choice.selected, Protocol.POWER_POWER_OFF, "wraps around");
        choice.key(Qt.Key_S, "s", false);
        compare(runs.count, 0, "suspend can't run");
        compare(choice.reason(Protocol.POWER_SUSPEND), "Turned off on this system");
    }

    function test_nothing_else_runs_meanwhile() {
        const choice = make();
        choice.pick(Protocol.POWER_SUSPEND);
        choice.pick(Protocol.POWER_LOCK);
        choice.key(Qt.Key_Down, "", false);
        compare(runs.count, 1);
        compare(choice.selected, Protocol.POWER_SUSPEND);
        choice.finished("logind didn't answer Suspend within 5 s");
        compare(choice.running, "");
        compare(choice.problem, "logind didn't answer Suspend within 5 s");
        choice.reset();
        compare(choice.problem, "");
    }

    function test_a_held_key_never_confirms() {
        const choice = make();
        verify(choice.key(Qt.Key_P, "p", false));
        verify(choice.key(Qt.Key_P, "p", true));
        verify(choice.key(Qt.Key_Return, "\r", true));
        compare(runs.count, 0);
        compare(choice.armed, Protocol.POWER_POWER_OFF);
        choice.key(Qt.Key_Up, "", true);
        compare(choice.selected, Protocol.POWER_REBOOT);
    }

    function test_unhandled_keys_pass_through() {
        verify(!make().key(Qt.Key_X, "x", false));
    }
}
