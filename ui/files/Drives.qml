pragma ComponentBehavior: Bound

import QtQuick
import "service"
import "paths.js" as Paths

// The drives in Nova Files' sidebar, as novad lists them. One that comes
// while the window is open (a stick plugged in) arrives with an animation.
// `folder` is the window's, so the drive it's on shows as current.
Column {
    id: drives

    property string folder: ""

    signal opened(string folder)
    signal menuRequested(var drive, point at)

    // The ids listed so far, and whether the window is past opening: rows
    // are made again on every change, and only a new one animates.
    property var seen: ({})
    property bool listed: false

    // The mounted drive the folder is on: the one with the longest mount
    // that holds it, since one may be mounted inside another (/ and /mnt/games).
    readonly property string currentId: {
        let best = null;
        for (const drive of FilesService.drives)
            if (drive.mount !== undefined && Paths.inside(folder, drive.mount) && (best === null || drive.mount.length > best.mount.length))
                best = drive;
        return best?.id ?? "";
    }

    Repeater {
        model: FilesService.drives

        DriveRow {
            required property var modelData

            width: drives.width
            drive: modelData
            current: modelData.id === drives.currentId
            arrived: drives.listed && !(modelData.id in drives.seen)
            Component.onCompleted: drives.seen[modelData.id] = true
            onOpened: FilesService.mount(modelData.id, mounted => {
                if (mounted !== null)
                    drives.opened(mounted);
            })
            onMenuRequested: at => drives.menuRequested(modelData, at)
        }
    }

    // Drives already there when the window opens don't animate: novad sends
    // the list as the process subscribes, before any window.
    Component.onCompleted: Qt.callLater(() => listed = true)
}
