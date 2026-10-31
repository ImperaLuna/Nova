pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// Quick settings' link to novad's Bluetooth: the radio, the devices and a
// pairing question for the tile, the page and the pairing pop-up, and the
// commands they send. novad does the work: one connect pairs, trusts and
// connects a new device.
Singleton {
    id: root

    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_BLUETOOTH)
    // The latest bluetooth event's data, or null (feature off, or none yet).
    readonly property var state: ready ? Daemon.bluetooth : null
    readonly property var devices: state?.devices ?? []
    // The question pairing waits on ({id, address, ask, incoming}), or null.
    readonly property var pairing: state?.pairing ?? null

    // The Bluetooth page shows on some screen: novad looks for nearby
    // devices meanwhile, and the page asks pairing questions in place of
    // the pop-up. Sent again after novad restarts, since a new connection
    // watches nothing.
    property bool watching: false
    onWatchingChanged: send()
    readonly property bool connected: Daemon.connected
    onConnectedChanged: {
        if (connected && watching)
            send();
    }

    function send() {
        Daemon.call(Protocol.CMD_WATCH_BLUETOOTH, {
            watching: watching
        });
    }

    function setPower(on: bool) {
        Daemon.call(Protocol.CMD_BLUETOOTH_POWER, {
            on: on
        });
    }

    // Pairs and trusts it first when it's new.
    function connectDevice(address: string) {
        Daemon.call(Protocol.CMD_BLUETOOTH_CONNECT, {
            address: address
        });
    }

    function disconnectDevice(address: string) {
        Daemon.call(Protocol.CMD_BLUETOOTH_DISCONNECT, {
            address: address
        });
    }

    function forget(address: string) {
        Daemon.call(Protocol.CMD_BLUETOOTH_FORGET, {
            address: address
        });
    }

    // Answers pairing question `id`: "accept", "reject" or {code}.
    function answer(id: int, answer: var) {
        Daemon.call(Protocol.CMD_BLUETOOTH_ANSWER, {
            id: id,
            answer: answer
        });
    }
}
