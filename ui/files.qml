pragma ComponentBehavior: Bound

//@ pragma AppId nova-files

import QtQuick
import Quickshell
import "daemon"
import "files"
import "files/service"
import "daemon/protocol.js" as Protocol

// Nova Files' own Quickshell process, apart from the shell's: novad's
// files module starts it (as NovaFiles) when a window is asked for and
// none runs. It opens a window for each one novad sends, and quits when
// its last window closes. It subscribes only to what its windows need.
ShellRoot {
    id: root

    property int windows: 0

    Component.onCompleted: Daemon.topics = [Protocol.TOPIC_FEATURES, Protocol.TOPIC_THEME, Protocol.TOPIC_FILES]

    Component {
        id: windowComponent
        FilesWindow {}
    }

    Connections {
        target: FilesService
        function onWindowRequested(window: var) {
            const shown = windowComponent.createObject(root, {
                folder: window.folder,
                select: window.select ?? []
            });
            root.windows++;
            console.info("nova: files window shown");
            shown.closed.connect(() => {
                shown.destroy();
                if (--root.windows === 0)
                    Qt.quit();
            });
        }
    }
}
