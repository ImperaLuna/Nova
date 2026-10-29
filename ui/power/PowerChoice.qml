import QtQuick
import "../theme"
import "../daemon/protocol.js" as Protocol

// What the power menu's clicks and keys pick, apart from drawing it. One row
// is selected at a time; it follows the mouse and the arrow keys and starts
// on Lock, the harmless one. Lock and suspend run at once. Log out, restart
// and shut down ask once more: a second pick within Tokens.durationConfirm
// runs them, otherwise they disarm. Nothing else is picked while one runs.
// Those three end the session, so they wait for the menu to close first
// (`leaving`): run while it closes, the session went down mid-animation and
// left the half-closed menu frozen on screen (IMP-200).
QtObject {
    id: choice

    // In the menu's order; `key` jumps to the row (and picks it).
    readonly property var actions: [
        {
            action: Protocol.POWER_LOCK,
            label: qsTr("Lock"),
            doing: qsTr("Locking"),
            key: "l",
            confirm: false,
            leaves: false
        },
        {
            action: Protocol.POWER_SUSPEND,
            label: qsTr("Suspend"),
            doing: qsTr("Suspending"),
            key: "s",
            confirm: false,
            leaves: false
        },
        {
            action: Protocol.POWER_LOG_OUT,
            label: qsTr("Log out"),
            doing: qsTr("Logging out"),
            key: "o",
            confirm: true,
            leaves: true
        },
        {
            action: Protocol.POWER_REBOOT,
            label: qsTr("Restart"),
            doing: qsTr("Restarting"),
            key: "r",
            confirm: true,
            leaves: true
        },
        {
            action: Protocol.POWER_POWER_OFF,
            label: qsTr("Shut down"),
            doing: qsTr("Shutting down"),
            key: "p",
            confirm: true,
            leaves: true
        }
    ]

    // novad's {action: {available, reason}}. An action it hasn't answered
    // for yet counts as available; novad checks again when it runs.
    property var options: ({})
    property string selected: Protocol.POWER_LOCK
    // The action waiting for its second pick, and how much of its wait is
    // left (1 down to 0).
    property string armed: ""
    property real armedLeft: 0
    // The action novad is running, or that waits for the menu to close.
    property string running: ""
    // The action that ends the session, waiting for the menu to close and
    // that to be on screen (left()).
    property string leaving: ""
    // Why the last action failed, until the menu opens again.
    property string problem: ""
    // Whether the wait is draining.
    readonly property bool counting: drain.running

    // An action to run now.
    signal runRequested(string action)
    // An action that ends the session: close the menu, then call left().
    signal leaveRequested(string action)
    // Esc with nothing armed.
    signal closeRequested

    function available(action: string): bool {
        return options[action]?.available ?? true;
    }

    function reason(action: string): string {
        return options[action]?.reason ?? "";
    }

    function leaves(action: string): bool {
        return actions.find(entry => entry.action === action)?.leaves ?? false;
    }

    // The menu opened. One opened again before it closed for an action
    // that ends the session keeps the session.
    function reset() {
        if (leaving !== "") {
            leaving = "";
            running = "";
        }
        disarm();
        selected = Protocol.POWER_LOCK;
        problem = "";
    }

    // The mouse went over a row.
    function hover(action: string) {
        if (armed === "" && running === "" && available(action))
            selected = action;
    }

    // Moves the selection `by` rows, wrapping and skipping what can't run.
    function step(by: int) {
        if (running !== "")
            return;
        disarm();
        const at = actions.findIndex(entry => entry.action === selected);
        for (let i = 1; i <= actions.length; i++) {
            const next = actions[(at + by * i + actions.length * i) % actions.length].action;
            if (available(next)) {
                selected = next;
                return;
            }
        }
    }

    function pick(action: string) {
        if (running !== "" || !available(action))
            return;
        selected = action;
        const entry = actions.find(entry => entry.action === action);
        if (entry.confirm && armed !== action) {
            armed = action;
            drain.restart();
            return;
        }
        disarm();
        running = action;
        if (entry.leaves) {
            leaving = action;
            leaveRequested(action);
            return;
        }
        runRequested(action);
    }

    // The menu has closed, and that's on screen: the action waiting for it
    // runs.
    function left() {
        if (leaving === "")
            return;
        const action = leaving;
        leaving = "";
        runRequested(action);
    }

    function disarm() {
        drain.stop();
        armed = "";
        armedLeft = 0;
    }

    // novad's answer to the running action: null, or why it failed.
    function finished(error: var) {
        running = "";
        problem = error ?? "";
    }

    // A key press in the open menu; whether it was the menu's.
    function key(key: int, text: string, repeat: bool): bool {
        // A held key repeats: it may still move the selection, but never
        // picks, so holding P can't both arm and confirm shutting down.
        const moves = [Qt.Key_Down, Qt.Key_Tab, Qt.Key_Up, Qt.Key_Backtab];
        if (repeat && !moves.includes(key))
            return true;
        if (key === Qt.Key_Escape) {
            if (armed !== "")
                disarm();
            else
                closeRequested();
            return true;
        }
        if (key === Qt.Key_Down || key === Qt.Key_Tab) {
            step(1);
            return true;
        }
        if (key === Qt.Key_Up || key === Qt.Key_Backtab) {
            step(-1);
            return true;
        }
        if (key === Qt.Key_Return || key === Qt.Key_Enter) {
            pick(selected);
            return true;
        }
        const entry = actions.find(entry => entry.key === text.toLowerCase());
        if (entry === undefined)
            return false;
        pick(entry.action);
        return true;
    }

    property NumberAnimation drain: NumberAnimation {
        target: choice
        property: "armedLeft"
        from: 1
        to: 0
        duration: Tokens.durationConfirm
        onFinished: choice.disarm()
    }
}
