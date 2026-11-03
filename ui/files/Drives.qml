pragma ComponentBehavior: Bound

import QtQuick
import "service"
import "paths.js" as Paths
import "space.js" as Space

// The drives in Nova Files' sidebar, as novad lists them. One that comes
// while the window is open (a stick plugged in) arrives with an animation.
// `folder` is the window's, so the drive it's on shows as current; when
// that drive goes away (ejected from any window, or pulled), `driveGone` says
// the window should go elsewhere. A drive clicked is opened by the window
// (`opened`), in the tab it shows.
Column {
    id: drives

    property string folder: ""
    // The drive `id` was clicked: open it (Navigator.openDrive).
    signal opened(string id)
    signal menuRequested(var drive, point at)
    signal driveGone

    // The ids listed so far, and whether the first list is in: rows are
    // made again on every change, and only a new one animates.
    property var seen: ({})
    property bool listed: false

    // The mounted drive the folder is on.
    readonly property var current: Space.driveOf(FilesService.drives, folder)
    // Where the drive the folder is on was mounted, before the latest list.
    property string heldMount: ""

    Connections {
        target: FilesService
        function onDrivesChanged() {
            const held = drives.heldMount;
            const gone = held !== "" && !FilesService.drives.some(drive => drive.mount === held);
            if (gone && Paths.inside(drives.folder, held))
                drives.driveGone();
            drives.heldMount = drives.current?.mount ?? "";
            if (!drives.listed)
                Qt.callLater(() => drives.listed = true);
        }
    }
    onFolderChanged: heldMount = current?.mount ?? ""

    Repeater {
        model: FilesService.drives

        DriveRow {
            required property var modelData

            width: drives.width
            drive: modelData
            current: modelData.id === (drives.current?.id ?? "")
            arrived: drives.listed && !(modelData.id in drives.seen)
            Component.onCompleted: drives.seen[modelData.id] = true
            onOpened: drives.opened(modelData.id)
            onMenuRequested: at => drives.menuRequested(modelData, at)
        }
    }

    // Drives in the first list the window sees don't animate, whether it was
    // there as the window opened or came after (udisksd starting slowly).
    Component.onCompleted: {
        heldMount = current?.mount ?? "";
        if (FilesService.drives.length > 0)
            Qt.callLater(() => listed = true);
    }
}
