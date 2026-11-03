pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// The launcher's link to novad: the apps and the user's pinned and recent
// ones, searching its providers, running what they found, pinning, and
// whether a launcher is open. The launcher (../Launcher.qml)
// gets these through the stage and never talks to Daemon itself.
Singleton {
    id: root

    // The latest launcher event's data ({apps, pinned, recent}), or null.
    readonly property var current: Daemon.launcher
    // The feature is on and novad has said what's installed.
    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_LAUNCHER) && current !== null

    // Nova Files runs, so the launcher has its Files mode.
    readonly property bool filesReady: Daemon.readyFeatures.includes(Protocol.FEATURE_FILES)

    // The screens whose launcher is open. novad hears when the first
    // opens and the last closes, and keeps the file index meanwhile.
    property var shownOn: ({})

    function setShown(output: string, shown: bool) {
        const was = Object.keys(shownOn).length > 0;
        const next = Object.assign({}, shownOn);
        if (shown)
            next[output] = true;
        else
            delete next[output];
        shownOn = next;
        const now = Object.keys(next).length > 0;
        if (now !== was)
            Daemon.call(Protocol.CMD_LAUNCHER_SHOWN, {
                shown: now
            });
    }

    // A novad started again hasn't heard of a launcher already open.
    Connections {
        target: Daemon
        function onConnectedChanged() {
            if (Daemon.connected && Object.keys(root.shownOn).length > 0)
                Daemon.call(Protocol.CMD_LAUNCHER_SHOWN, {
                    shown: true
                });
        }
    }

    // `done(items)` gets the LauncherItems novad's providers found for
    // `query` in the launcher's `mode` (a provider's id; the apps mode
    // mixes every provider without a mode of its own), or none when the
    // search failed.
    function search(query: string, mode: string, done: var) {
        Daemon.call(Protocol.CMD_LAUNCHER_SEARCH, {
            query: query,
            mode: mode === Protocol.PROVIDER_APPS ? null : mode
        }, (error, result) => {
            if (error)
                console.warn(`nova: launcher search: ${error}`);
            done(error ? [] : result);
        });
    }

    // Runs what `provider` found as `id` (on the screen `output`): an app
    // starts, or its open window is focused where it is unless `newWindow`.
    // `done(error)` gets novad's answer.
    function run(provider: string, id: string, output: string, newWindow: bool, done: var) {
        Daemon.call(Protocol.CMD_LAUNCHER_RUN, {
            provider: provider,
            id: id,
            output: output,
            new_window: newWindow
        }, error => done(error ?? null));
    }

    function pin(id: string, pinned: bool) {
        Daemon.call(Protocol.CMD_PIN_APP, {
            id: id,
            pinned: pinned
        });
    }
}
