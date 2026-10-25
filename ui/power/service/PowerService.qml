pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// The power menu's link to novad: what it can do now, asked each time it
// opens, and running an action. The menu (../PowerMenu.qml) acts through
// here, never through Daemon directly.
Singleton {
    id: root

    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_POWER)
    // novad's last answer, {action: {available, reason}}; empty until then.
    property var options: ({})

    function refresh() {
        Daemon.call(Protocol.CMD_POWER_OPTIONS, undefined, (error, result) => {
            if (error) {
                console.warn(`nova: power options: ${error}`);
                return;
            }
            const byAction = {};
            for (const option of result)
                byAction[option.action] = option;
            root.options = byAction;
        });
    }

    // Runs a Protocol.POWER_* action; `done(error)` gets novad's answer.
    function run(action: string, done: var) {
        Daemon.call(Protocol.CMD_POWER, {
            action: action
        }, error => done(error ?? null));
    }
}
