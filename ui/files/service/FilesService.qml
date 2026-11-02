pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// Nova Files' link to novad: the windows it asks for. The Nova Files
// process (../../files.qml) and its windows get these here and never talk
// to Daemon themselves.
Singleton {
    id: root

    // A window to open on `window.folder`, with `window.select` (absolute
    // paths in it, maybe none) selected.
    signal windowRequested(var window)

    // Tells novad this process opens Nova Files' windows; done again on
    // every connection.
    function attach() {
        Daemon.call(Protocol.CMD_ATTACH_FILES, {
            attached: true
        });
    }

    // Tells novad to send no more windows, then calls `done`, even when
    // novad isn't there.
    function detach(done) {
        Daemon.call(Protocol.CMD_ATTACH_FILES, {
            attached: false
        }, () => done());
    }

    Connections {
        target: Daemon
        function onFilesWindowRequested(window: var) {
            root.windowRequested(window);
        }
        function onConnectedChanged() {
            if (Daemon.connected)
                root.attach();
        }
    }
}
