import QtQuick
import "../theme"
import "../daemon/protocol.js" as Protocol

// What the power menu's clicks and keys pick, apart from drawing it. One row
// is selected at a time; it follows the mouse and the arrow keys and starts
// on Lock, the harmless one. Lock and suspend run at once. Log out, restart
// and shut down ask once more: a second pick within Tokens.durationConfirm
// runs them, otherwise they disarm. Nothing else is picked while one runs.
QtObject {
    id: choice

    // In the menu's order; `key` jumps to the row (and picks it).
    readonly property var actions: [
        {
            action: Protocol.POWER_LOCK,
            label: qsTr("Lock"),
            doing: qsTr("Locking"),
            key: "l",
            confirm: false
        },
        {
            action: Protocol.POWER_SUSPEND,
            label: qsTr("Suspend"),
            doing: qsTr("Suspending"),
            key: "s",
            confirm: false
        },
        {
            action: Protocol.POWER_LOG_OUT,
            label: qsTr("Log out"),
            doing: qsTr("Logging out"),
            key: "o",
            confirm: true
        },
        {
            action: Protocol.POWER_REBOOT,
            label: qsTr("Restart"),
            doing: qsTr("Restarting"),
            key: "r",
            confirm: true
        },
        {
            action: Protocol.POWER_POWER_OFF,
            label: qsTr("Shut down"),
            doing: qsTr("Shutting down"),
            key: "p",
            confirm: true
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
    // The action novad is running.
    property string running: ""
    // Why the last action failed, until the menu opens again.
    property string problem: ""
    // Whether the wait is draining.
    readonly property bool counting: drain.running

    // An action to run now.
    signal runRequested(string action)
    // Esc with nothing armed.
    signal closeRequested

    function available(action: string): bool {
        return options[action]?.available ?? true;
    }

    function reason(action: string): string {
        return options[action]?.reason ?? "";
    }

    // The menu opened.
    function reset() {
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
    function key(key: int, text: string): bool {
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
