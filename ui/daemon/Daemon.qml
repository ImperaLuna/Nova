pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import "protocol.js" as Protocol

// The UI's one connection to novad. Reconnects on its own, exposes daemon
// state as properties, and routes replies back to `call` callbacks.
Singleton {
    id: root

    readonly property bool connected: socket.connected
    property string version: ""
    property var features: []
    readonly property var readyFeatures: Protocol.readyFeatures(features)

    property int _nextId: 1
    property var _pending: ({})

    function call(cmd, args, callback) {
        if (!socket.connected)
            return false;
        const id = _nextId++;
        if (callback)
            _pending[id] = callback;
        socket.write(Protocol.request(id, cmd, args));
        socket.flush();
        return true;
    }

    function _handle(line) {
        const msg = Protocol.parse(line);
        if (!msg) {
            console.warn("nova: ignoring message from novad:", line);
            return;
        }
        if (msg.kind === "response") {
            const callback = _pending[msg.id];
            delete _pending[msg.id];
            if (callback)
                callback(msg.error, msg.result);
        } else if (msg.event === "hello") {
            version = msg.data.daemon;
            console.info(`nova: connected to novad ${version}`);
        } else if (msg.event === "features") {
            features = msg.data.features;
        }
    }

    Socket {
        id: socket

        path: {
            const override = Quickshell.env("NOVA_SOCKET");
            if (override)
                return override;
            const runtime = Quickshell.env("XDG_RUNTIME_DIR");
            return runtime ? `${runtime}/nova/novad.sock` : "";
        }
        connected: true

        parser: SplitParser {
            onRead: data => root._handle(data)
        }

        onConnectionStateChanged: {
            if (!connected) {
                root._pending = {};
                retry.start();
            }
        }
    }

    // novad may start after the UI, or restart. Keep trying quietly.
    Timer {
        id: retry
        interval: 1000
        onTriggered: socket.connected = true
    }
}
