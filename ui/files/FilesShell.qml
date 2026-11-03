pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../daemon"
import "service"
import "../daemon/protocol.js" as Protocol

// A Nova Files process, apart from the shell's (../files.qml for Nova
// Files' own windows, ../files-picker.qml for apps' Open and Save dialogs):
// novad's files module starts it when a window for it is asked for and
// none runs. Once connected it attaches, opens a window for each one novad
// sends, and quits when its last window closes. It subscribes only to what
// its windows need.
ShellRoot {
    id: root

    // The dialogs' process: novad sends it only pickers.
    property bool pickers: false
    property int windows: 0

    Component.onCompleted: {
        FilesService.pickers = pickers;
        Daemon.topics = [Protocol.TOPIC_FEATURES, Protocol.TOPIC_THEME, Protocol.TOPIC_FILES];
    }

    // Quits once novad sends no more windows, unless one came meanwhile.
    function quit() {
        FilesService.detach(() => {
            if (windows === 0)
                Qt.quit();
            else
                FilesService.attach();
        });
    }

    Component {
        id: windowComponent
        FilesWindow {}
    }

    Connections {
        target: FilesService
        function onWindowRequested(window: var) {
            const shown = windowComponent.createObject(root, {
                folder: window.folder,
                select: window.select ?? [],
                picker: window.picker ?? null
            });
            root.windows++;
            console.info("nova: files window shown");
            shown.closed.connect(() => {
                shown.destroy();
                if (--root.windows === 0)
                    root.quit();
            });
        }
    }
}
