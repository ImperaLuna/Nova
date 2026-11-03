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
    // and so for good on compositors novad doesn't know, which send none.
    // Read it through the Compositor singleton.
    property var desktop: null
    // The latest launcher event's data ({apps, pinned, recent}); null until
    // one arrives, which only happens with the launcher feature on. Read it
    // through the LauncherService singleton.
    property var launcher: null
    // The latest tray event's data ({items, hidden}); null until one
    // arrives, which only happens with the tray feature on. Read it through
    // the TrayService singleton.
    property var tray: null
    // The latest media event's data ({players, shown}); null until one
    // arrives, which only happens with the media feature on. Read it through
    // the MediaService singleton.
    property var media: null
    // The latest files_look event's data ({frame, list}); null until one
    // arrives, which only happens with the files feature on. Read it
    // through the FilesService singleton.
    property var filesLook: null
    readonly property var readyFeatures: Protocol.readyFeatures(features)
    // novad's word on whether the session is locked. Read it through the
    // LockService singleton, which also hears lockRequested.
    property bool locked: false
    // The latest host event's data ({host, user, booted_at, os}) and
    // resources event's, null until each arrives. Read them through the
    // QuickSettingsService singleton.
    property var host: null
    property var resources: null
    // The latest peripherals event's data ({devices}); null until one
    // arrives, which only happens with the peripherals feature on. Read it
    // through the QuickSettingsService singleton.
    property var peripherals: null
    // The latest network event's data ({running, primary, prefer, wired,
    // wifi}) and network_traffic event's ({down, up, today_down,
    // today_up}); null until each arrives, which only happens with the
    // network feature on. Read them through the NetworkService singleton.
    property var network: null
    property var networkTraffic: null

    // The latest bluetooth event's data ({radio, scanning, devices,
    // pairing}); null until one arrives, which only happens with the
    // bluetooth feature on. Read it through the BluetoothService singleton.
    property var bluetooth: null
    // The latest notifications event's data ({items, do_not_disturb, muted,
    // apps}); null until one arrives, which only happens with the
    // notifications feature on. Read it, and hear notificationPopped,
    // through the NotificationsService singleton.
    property var notifications: null
    // A notification to pop up (one of `notifications.items`' shape).
    signal notificationPopped(var notification)

    // `nova toggle <panel>`: open or close the StagePanel named `panel`.
    // `output` is the focused output's name, or null when novad doesn't
    // know it.
    signal toggleRequested(string panel, var output)
    // A lock event said to lock the screen.
    signal lockRequested
    // `nova volume <key>`: a Protocol.VOLUME_* key, for the volume capsule.
    signal volumeRequested(string key)
    // The region picker's frozen frames while it's open ([{output, path}],
    // one per screen), or null.
    property var capture: null
    // A screenshot was saved: {id, path, width, height, copied}.
    signal screenshotSaved(var shot)
    // `nova record` with nothing recording: open or close the picker.
    signal recordingPickRequested
    // What's being recorded ({output, region, audio, elapsed_ms, paused,
    // can_pause}), or null.
    property var recording: null
    // A recording was saved: {id, path, duration_ms}.
    signal recordingSaved(var saved)
    // A Nova Files window to open: {folder, select}. Only the Nova Files
    // process subscribes to its topic.
    signal filesWindowRequested(var window)
    // A folder view's page of entries ({view, path, start, total,
    // entries}), its changes ({view, changes}), and novad closing it
    // ({view, error}). Only the client that opened the view gets these.
    signal filesPage(var page)
    signal filesChanged(var changed)
    signal filesClosed(var closed)
    // The drives Nova Files' sidebar lists ({id, name, removable, mount,
    // size, used} each), from the latest files_drives event. Read it
    // through FilesService.
    property var filesDrives: []
    // How one of this process's copy or move jobs is going: {job, mode,
    // state, items, name, done_bytes, total_bytes, clash, error}.
    signal filesTransferChanged(var transfer)
    // How far one of this process's measuring jobs got: {job, files,
    // folders, bytes, disk_bytes, done}.
    signal filesMeasured(var measured)
    // Something in the trash changed, in Nova Files or another app.
    signal filesTrashChanged

    // The topics this process subscribes to on every connection: the
    // shell's, unless its root sets its own as it's created (Nova Files,
    // ../files.qml), before the first connection. Topics of features that
    // are off send nothing.
    property var topics: [Protocol.TOPIC_FEATURES, Protocol.TOPIC_THEME, Protocol.TOPIC_AI_USAGE, Protocol.TOPIC_COMPOSITOR, Protocol.TOPIC_TOGGLE, Protocol.TOPIC_LOCK, Protocol.TOPIC_VOLUME, Protocol.TOPIC_SCREENSHOT, Protocol.TOPIC_RECORDING, Protocol.TOPIC_HOST, Protocol.TOPIC_RESOURCES, Protocol.TOPIC_PERIPHERALS, Protocol.TOPIC_NETWORK, Protocol.TOPIC_BLUETOOTH, Protocol.TOPIC_LAUNCHER, Protocol.TOPIC_TRAY, Protocol.TOPIC_NOTIFICATIONS, Protocol.TOPIC_MEDIA]

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
        } else if (msg.event === Protocol.EVENT_CAPTURE) {
            capture = msg.data.frames;
        } else if (msg.event === Protocol.EVENT_CAPTURE_ENDED) {
            capture = null;
        } else if (msg.event === Protocol.EVENT_SCREENSHOT_SAVED) {
            screenshotSaved(msg.data);
        } else if (msg.event === Protocol.EVENT_PICK_RECORDING) {
            recordingPickRequested();
        } else if (msg.event === Protocol.EVENT_RECORDING) {
            recording = msg.data;
        } else if (msg.event === Protocol.EVENT_RECORDING_SAVED) {
            recordingSaved(msg.data);
        } else if (msg.event === Protocol.EVENT_HOST) {
            host = msg.data;
        } else if (msg.event === Protocol.EVENT_RESOURCES) {
            resources = msg.data;
        } else if (msg.event === Protocol.EVENT_PERIPHERALS) {
            peripherals = msg.data;
        } else if (msg.event === Protocol.EVENT_NETWORK) {
            network = msg.data;
        } else if (msg.event === Protocol.EVENT_NETWORK_TRAFFIC) {
            networkTraffic = msg.data;
        } else if (msg.event === Protocol.EVENT_BLUETOOTH) {
            bluetooth = msg.data;
        } else if (msg.event === Protocol.EVENT_LAUNCHER) {
            launcher = msg.data;
        } else if (msg.event === Protocol.EVENT_TRAY) {
            tray = msg.data;
        } else if (msg.event === Protocol.EVENT_NOTIFICATIONS) {
            notifications = msg.data;
        } else if (msg.event === Protocol.EVENT_NOTIFICATION_POPUP) {
            notificationPopped(msg.data);
        } else if (msg.event === Protocol.EVENT_MEDIA) {
            media = msg.data;
        } else if (msg.event === Protocol.EVENT_FILES_WINDOW) {
            filesWindowRequested(msg.data);
        } else if (msg.event === Protocol.EVENT_FILES_PAGE) {
            filesPage(msg.data);
        } else if (msg.event === Protocol.EVENT_FILES_CHANGED) {
            filesChanged(msg.data);
        } else if (msg.event === Protocol.EVENT_FILES_CLOSED) {
            filesClosed(msg.data);
        } else if (msg.event === Protocol.EVENT_FILES_LOOK) {
            filesLook = msg.data;
        } else if (msg.event === Protocol.EVENT_FILES_DRIVES) {
            filesDrives = msg.data.drives;
        } else if (msg.event === Protocol.EVENT_FILES_TRANSFER) {
            filesTransferChanged(msg.data);
        } else if (msg.event === Protocol.EVENT_FILES_MEASURED) {
            filesMeasured(msg.data);
        } else if (msg.event === Protocol.EVENT_FILES_TRASH_CHANGED) {
            filesTrashChanged();
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
        // Connected once whatever made this singleton is created, so its
        // root has set `topics` first.
        Component.onCompleted: Qt.callLater(() => connected = true)

        parser: SplitParser {
            onRead: data => root._handle(data)
        }

        onConnectionStateChanged: {
            if (connected) {
                // Only hello arrives unasked; everything else is subscribed to.
                root.call(Protocol.CMD_SUBSCRIBE, {
                    topics: root.topics
                });
            } else {
                // A picker novad still has open is sent again on reconnect.
                root.capture = null;
                root.recording = null;
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
