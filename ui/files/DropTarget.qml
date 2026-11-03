pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "drops.js" as Drops

// Where Nova Files takes files dragged onto it, from its own windows or
// from other apps (text/uri-list). `folderAt(x, y)` names the place under
// the pointer (Drops.place, or Drops.NOWHERE); while a drag is over one,
// `hovered` and `action` say what a drop there would do (Drops.decide),
// asking novad once per folder whether it's on the dragged items' drive,
// and the pointer shows it, or that nothing can be dropped there. A drop
// asks the window to do it (`dropRequested`); a place that springs opens
// after the pointer rests on it (`springOpened`).
DropArea {
    id: target

    required property var folderAt
    // What asks novad which drive things are on: FilesService, or a
    // stand-in in tests.
    required property var service

    // The place under the pointer while a drag is over, or null.
    property var hovered: null
    readonly property string action: hovered === null ? Drops.NONE : Drops.decide(sources, hovered.folder, hovered.trash, copy, sameDrives[hovered.folder] ?? null)

    // The drag's items (absolute paths), whether it asks for a copy, and
    // for each folder novad answered about, whether they're on its drive.
    property var sources: []
    property bool copy: false
    property var sameDrives: ({})

    signal dropRequested(string action, var sources, string folder)
    signal springOpened(string folder)

    keys: [Drops.URI_LIST]

    // Follows the pointer to (x, y); the pointer shows what a drop there does.
    function follow(drag: var) {
        const place = folderAt(drag.x, drag.y);
        if (place?.folder !== hovered?.folder || place?.trash !== hovered?.trash) {
            hovered = place;
            spring.restart();
            if (place !== null && !place.trash && !(place.folder in sameDrives))
                ask(place.folder);
        }
        drag.accepted = action !== Drops.NONE;
        if (drag.accepted)
            drag.action = Drops.dragAction(action);
    }

    function ask(folder: string) {
        const asked = sources;
        target.service.sameDrive(asked, folder, same => {
            if (asked !== target.sources || same === null)
                return;
            target.sameDrives = Object.assign({}, target.sameDrives, {
                [folder]: same
            });
        });
    }

    function leave() {
        hovered = null;
        spring.stop();
    }

    // A drag came in: `drag` is the DragEvent, or anything with its x, y,
    // urls, proposedAction, accepted and action.
    function enter(drag: var) {
        sources = Drops.localPaths(drag.urls);
        // Ctrl turns the proposed action into a copy; anything else lets
        // the drive decide.
        copy = drag.proposedAction === Qt.CopyAction;
        sameDrives = {};
        if (sources.length === 0) {
            drag.accepted = false;
            return;
        }
        follow(drag);
        // A refused place still follows the pointer to the next one.
        drag.accepted = true;
        if (action === Drops.NONE)
            drag.action = Qt.IgnoreAction;
    }

    // The drag was let go: `drop` is the DragEvent, or anything with its
    // accepted and accept().
    function release(drop: var) {
        const place = hovered;
        const action = target.action;
        const sources = target.sources;
        leave();
        if (place === null || action === Drops.NONE) {
            drop.accepted = false;
            return;
        }
        drop.accept(Drops.dragAction(action));
        if (action === Drops.TRASH || place.folder in sameDrives || copy) {
            dropRequested(action, sources, place.folder);
            return;
        }
        // Dropped before novad said which drive: ask, then do it.
        target.service.sameDrive(sources, place.folder, same => {
            const settled = Drops.decide(sources, place.folder, false, false, same ?? false);
            if (settled !== Drops.NONE)
                target.dropRequested(settled, sources, place.folder);
        });
    }

    onEntered: drag => enter(drag)
    onPositionChanged: drag => follow(drag)
    onExited: leave()
    onDropped: drop => release(drop)

    Timer {
        id: spring
        interval: Tokens.durationFilesSpring
        onTriggered: {
            if (target.hovered?.spring && target.action !== Drops.NONE)
                target.springOpened(target.hovered.folder);
        }
    }
}
