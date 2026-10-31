pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// The network tile's and page's link to novad: the links and Wi-Fi
// networks NetworkManager has, traffic while the page shows it, and the
// commands. The sidebar (../QuickSettings.qml) reads and acts through here.
Singleton {
    id: root

    // The feature is on and novad has reported: the tile shows.
    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_NETWORK) && network !== null
    // The latest network event's data ({running, primary, prefer, wired,
    // wifi}), or null.
    readonly property var network: Daemon.network
    // The latest network_traffic event's data ({down, up, today_down,
    // today_up}), or null.
    readonly property var traffic: Daemon.networkTraffic

    // Whether traffic is wanted every second (the page shows it). Sent again
    // after novad restarts, since a new connection watches nothing.
    property bool watching: false
    onWatchingChanged: sendWatching()
    readonly property bool connected: Daemon.connected
    onConnectedChanged: {
        if (connected && watching)
            sendWatching();
    }

    function setWifi(enabled: bool) {
        Daemon.call(Protocol.CMD_SET_WIFI, {
            enabled: enabled
        });
    }

    function scan() {
        Daemon.call(Protocol.CMD_SCAN_WIFI);
    }

    // Joins `ssid`, with `password` for a network that isn't saved (null
    // otherwise). How it goes comes back as the network event's
    // wifi.attempt.
    function connect(ssid: string, password: var) {
        Daemon.call(Protocol.CMD_CONNECT_WIFI, password === null ? {
            ssid: ssid
        } : {
            ssid: ssid,
            password: password
        });
    }

    function disconnect() {
        Daemon.call(Protocol.CMD_DISCONNECT_WIFI);
    }

    function forget(ssid: string) {
        Daemon.call(Protocol.CMD_FORGET_WIFI, {
            ssid: ssid
        });
    }

    // Which link carries traffic while both are connected: "wired" or
    // "wifi".
    function prefer(kind: string) {
        Daemon.call(Protocol.CMD_PREFER_NETWORK, {
            kind: kind
        });
    }

    function sendWatching() {
        Daemon.call(Protocol.CMD_WATCH_NETWORK_TRAFFIC, {
            watching: watching
        });
    }
}
