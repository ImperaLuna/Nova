pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol
import "../launcher.js" as LauncherLogic

// The launcher's link to novad: the apps and the user's pinned and recent
// ones, searching, launching and pinning. The launcher (../Launcher.qml)
// gets these through the stage and never talks to Daemon itself.
Singleton {
    id: root

    // The latest launcher event's data ({apps, pinned, recent}), or null.
    readonly property var current: Daemon.launcher
    // The feature is on and novad has said what's installed.
    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_LAUNCHER) && current !== null

    // `done(matches)` gets novad's AppMatches for `query`, best first, or
    // none when the search failed.
    function search(query: string, done: var) {
        Daemon.call(Protocol.CMD_SEARCH_APPS, {
            query: query
        }, (error, result) => {
            if (error)
                console.warn(`nova: launcher search: ${error}`);
            done(error ? [] : result);
        });
    }

    // Starts the app `id` (on the screen `output`), or focuses its open
    // window where it is unless `newWindow`; `done(error)` gets novad's
    // answer.
    function launch(id: string, output: string, newWindow: bool, done: var) {
        Daemon.call(Protocol.CMD_LAUNCH_APP, {
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

    // What an Image shows for an app's icon (a theme name or an absolute
    // path), or "" when the theme has no such icon.
    function iconSource(icon: string): string {
        if (!icon)
            return "";
        if (icon.startsWith("/"))
            return LauncherLogic.fileUrl(icon);
        return Quickshell.iconPath(icon, true);
    }
}
