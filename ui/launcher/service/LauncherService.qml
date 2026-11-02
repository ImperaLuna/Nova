pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// The launcher's link to novad: the apps and the user's pinned and recent
// ones, searching its providers, running what they found, and pinning. The launcher (../Launcher.qml)
// gets these through the stage and never talks to Daemon itself.
Singleton {
    id: root

    // The latest launcher event's data ({apps, pinned, recent}), or null.
    readonly property var current: Daemon.launcher
    // The feature is on and novad has said what's installed.
    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_LAUNCHER) && current !== null

    // `done(items)` gets the LauncherItems novad's providers found for
    // `query`, or none when the search failed.
    function search(query: string, done: var) {
        Daemon.call(Protocol.CMD_LAUNCHER_SEARCH, {
            query: query
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
