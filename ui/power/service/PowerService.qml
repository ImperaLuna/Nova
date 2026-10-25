pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// The power menu's link to novad: what it can do now and when the system
// booted, asked each time it opens, and running an action. The menu (../PowerMenu.qml) acts through
// here, never through Daemon directly.
Singleton {
    id: root

    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_POWER)
    // novad's last answer, {action: {available, reason}}; empty until then.
    property var options: ({})
    // When the system booted, as a time in ms like Date's; 0 until novad
    // says, or when it can't tell.
    property real bootedAt: 0

    function refresh() {
        Daemon.call(Protocol.CMD_POWER_OPTIONS, undefined, (error, result) => {
            if (error) {
                console.warn(`nova: power options: ${error}`);
                return;
            }
            const byAction = {};
            for (const option of result.options)
                byAction[option.action] = option;
            root.options = byAction;
            root.bootedAt = (result.booted_at ?? 0) * 1000;
        });
    }

    // Runs a Protocol.POWER_* action; `done(error)` gets novad's answer.
    function run(action: string, done: var) {
        Daemon.call(Protocol.CMD_POWER, {
            action: action
        }, error => done(error ?? null));
    }
}
