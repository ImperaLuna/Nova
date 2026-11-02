pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"

// Nova Files' link to novad: the windows it asks for. The Nova Files
// process (../../files.qml) and its windows get these here and never talk
// to Daemon themselves.
Singleton {
    id: root

    // A window to open on `window.folder`, with `window.select` (absolute
    // paths in it, maybe none) selected.
    signal windowRequested(var window)

    Connections {
        target: Daemon
        function onFilesWindowRequested(window: var) {
            root.windowRequested(window);
        }
    }
}
