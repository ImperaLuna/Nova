pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import "../theme"
import "protocol.js" as Protocol

// The UI's one connection to novad. Reconnects on its own, exposes daemon
// state as properties, and routes replies back to `call` callbacks.
Singleton {
    id: root

    readonly property bool connected: socket.connected
    property string version: ""
    property var features: []
    // The active theme event's data ({id, name, mode, colors}); null until
    // the first one arrives. Its colors go to Tokens.
    property var theme: null
    // The latest ai_usage event's data ({shown, providers}); null until one
    // arrives, which only happens with the ai-usage feature on. Read it
    // through the AiUsageService singleton.
    property var aiUsage: null
    // The latest compositor event's data ({compositor, capabilities,
    // outputs, focused_window, keyboard_layout}); null until one arrives,
    // and always on compositors novad doesn't know. Read it through the
    // Compositor singleton.
    property var desktop: null
    // The latest launcher event's data ({apps, pinned, recent}); null until
    // one arrives, which only happens with the launcher feature on. Read it
    // through the LauncherService singleton.
    property var launcher: null
    readonly property var readyFeatures: Protocol.readyFeatures(features)
    // novad's word on whether the session is locked. Read it through the
    // LockService singleton, which also hears lockRequested.
    property bool locked: false

    // `nova toggle <panel>`: open or close the StagePanel named `panel`.
    // `output` is the focused output's name, or null when novad doesn't
    // know it.
    signal toggleRequested(string panel, var output)
    // A lock event said to lock the screen.
    signal lockRequested
    // `nova volume <key>`: a Protocol.VOLUME_* key, for the volume capsule.
    signal volumeRequested(string key)

    property int _nextId: 1
    property var _pending: ({})

    // Sends a Protocol.CMD_* command. `callback(error, result)` gets novad's
    // reply, or an error when novad isn't there or goes away first. Without
    // one, an error is logged, so a failed command is never silent.
    function call(cmd, args, callback) {
        const reply = callback ?? ((error, result) => {
                if (error)
                    console.warn(`nova: ${cmd} failed: ${error}`);
            });
        if (!socket.connected) {
            reply("not connected to novad");
            return false;
        }
        const id = _nextId++;
        _pending[id] = reply;
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
            const reply = _pending[msg.id];
            delete _pending[msg.id];
            if (reply)
                reply(msg.error, msg.result);
        } else if (msg.event === Protocol.EVENT_HELLO) {
            version = msg.data.daemon;
            console.info(`nova: connected to novad ${version}`);
        } else if (msg.event === Protocol.EVENT_FEATURES) {
            features = msg.data.features;
        } else if (msg.event === Protocol.EVENT_THEME) {
            Tokens.apply(msg.data.colors);
            theme = msg.data;
        } else if (msg.event === Protocol.EVENT_AI_USAGE) {
            aiUsage = msg.data;
        } else if (msg.event === Protocol.EVENT_COMPOSITOR) {
            desktop = msg.data;
        } else if (msg.event === Protocol.EVENT_TOGGLE) {
            toggleRequested(msg.data.panel, msg.data.output ?? null);
        } else if (msg.event === Protocol.EVENT_LOCK) {
            locked = msg.data.locked;
            if (locked)
                lockRequested();
        } else if (msg.event === Protocol.EVENT_VOLUME) {
            volumeRequested(msg.data.key);
        } else if (msg.event === Protocol.EVENT_LAUNCHER) {
            launcher = msg.data;
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
            if (connected) {
                // Only hello arrives unasked; everything else is subscribed to.
                // Topics of features that are off send nothing.
                root.call(Protocol.CMD_SUBSCRIBE, {
                    topics: [Protocol.TOPIC_FEATURES, Protocol.TOPIC_THEME, Protocol.TOPIC_AI_USAGE, Protocol.TOPIC_COMPOSITOR, Protocol.TOPIC_TOGGLE, Protocol.TOPIC_LOCK, Protocol.TOPIC_VOLUME, Protocol.TOPIC_LAUNCHER]
                });
            } else {
                // Replies to these won't come any more.
                const pending = root._pending;
                root._pending = {};
                for (const id in pending)
                    pending[id]("novad went away");
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
