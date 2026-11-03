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
// none runs. Once connected it attaches, opens a window for each one novad
// sends (an app's Open or Save dialog too, as a window in picker mode,
// and each tab torn out of one), and quits when its last window closes.
// It subscribes only to what its windows need.
ShellRoot {
    id: root

    property int windows: 0

    Component.onCompleted: Daemon.topics = [Protocol.TOPIC_FEATURES, Protocol.TOPIC_THEME, Protocol.TOPIC_FILES]

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

    // Shows a window at `folder` with `select` selected, or as the tab
    // `carried` was (FilesWindow.carried), or as an app's file picker
    // (`picker`, a FilePicker).
    function show(folder: string, select: var, carried: var, picker: var) {
        const shown = windowComponent.createObject(root, {
            folder: folder,
            select: select,
            carried: carried,
            picker: picker ?? null
        });
        root.windows++;
        console.info("nova: files window shown");
        shown.windowRequested.connect((folder, select, carried) => root.show(folder, select, carried, null));
        shown.closed.connect(() => {
            shown.destroy();
            if (--root.windows === 0)
                root.quit();
        });
    }

    Connections {
        target: FilesService
        function onWindowRequested(window: var) {
            root.show(window.folder, window.select ?? [], null, window.picker ?? null);
        }
    }
}
