pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// Quick settings' link to novad: the machine and its user for the header,
// the resources card's numbers while the sidebar is open, and keep awake.
// The sidebar (../QuickSettings.qml) reads and acts through here.
Singleton {
    id: root

    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_QUICK_SETTINGS)
    // The latest host event's data ({host, user, booted_at, os}), or null.
    readonly property var host: Daemon.host
    // The latest resources event's data, or null until one has come.
    readonly property var resources: Daemon.resources

    // Keep awake: every screen's stage holds an idle inhibitor while it's
    // on. Lasts until the UI restarts.
    property bool keepAwake: false

    // Whether resources are wanted (the sidebar is open). Sent again after
    // novad restarts, since a new connection watches nothing.
    property bool watching: false
    onWatchingChanged: send()
    readonly property bool connected: Daemon.connected
    onConnectedChanged: {
        if (connected && watching)
            send();
    }
    function send() {
        Daemon.call(Protocol.CMD_WATCH_RESOURCES, {
            watching: watching
        });
    }
}
