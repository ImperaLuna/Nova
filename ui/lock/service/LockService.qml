pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// Whether the lock screen (../LockScreen.qml) covers the screens. novad's
// lock event locks it (`nova lock`, the power menu); only the password
// checking out here unlocks it, so nothing on the socket can.
Singleton {
    id: root

    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_LOCK)
    // Locked from novad's word until unlocked() runs.
    property bool locked: false

    // The password checked out: unlock, then tell novad.
    function unlocked() {
        locked = false;
        Daemon.call(Protocol.CMD_REPORT_UNLOCKED);
    }

    // A UI restarted while locked learns it from novad's first lock event,
    // which may have come before this singleton was made.
    Component.onCompleted: locked = Daemon.locked

    Connections {
        target: Daemon
        function onLockRequested() {
            root.locked = true;
        }
    }
}
