pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "drops.js" as Drops

// Where Nova Files takes files dragged onto it, from its own windows or
// from other apps (text/uri-list). `folderAt(x, y)` names the place under
// the pointer (Drops.place, or Drops.NOWHERE); while a drag is over one,
// `hovered` and `action` say what a drop there would do, as novad answers
// (files_drop, asked once per folder and Ctrl state; onto the trash it's
// Drops.TRASH), and the pointer shows it, or that nothing can be dropped
// there, also while novad hasn't answered. Ctrl is read again at every
// move and at the drop. A drop goes where it's let go, asks novad first
// if it hasn't answered for there, and then asks the window to do it
// (`dropRequested`); a place that springs opens after the pointer rests
// on it (`springOpened`).
DropArea {
    id: target

    required property var folderAt
    // What asks novad what a drop does: FilesService, or a stand-in in
    // tests.
    required property var service

    // The place under the pointer while a drag is over, or null.
    property var hovered: null
    readonly property string action: what(hovered, copy, answers)

    // The drag's items (absolute paths), whether it asks for a copy (Ctrl),
    // and novad's answers for it: Drops.NONE or a mode, by answerKey.
    property var sources: []
    property bool copy: false
    property var answers: ({})

    signal dropRequested(string action, var sources, string folder)
    signal springOpened(string folder)

    keys: [Drops.URI_LIST]

    function answerKey(folder: string, copy: bool): string {
        return (copy ? "copy:" : "drop:") + folder;
    }

    // What a drop on `place` does with `copy`, from `known` answers; nothing
    // while novad hasn't answered.
    function what(place: var, copy: bool, known: var): string {
        if (place === null)
            return Drops.NONE;
        if (place.trash)
            return Drops.TRASH;
        return known[answerKey(place.folder, copy)] ?? Drops.NONE;
    }

    // Asks novad what a drop on `folder` does with `copy`, then `done(action)`.
    function ask(folder: string, copy: bool, done: var) {
        const asked = sources;
        service.drop(asked, folder, copy, mode => {
            if (asked !== target.sources)
                return;
            const action = mode ?? Drops.NONE;
            target.answers = Object.assign({}, target.answers, {
                [target.answerKey(folder, copy)]: action
            });
            done?.(action);
        });
    }

    // Follows the drag to (x, y); the pointer shows what a drop there does.
    function follow(drag: var) {
        copy = drag.proposedAction === Qt.CopyAction;
        const place = folderAt(drag.x, drag.y);
        if (place?.folder !== hovered?.folder || place?.trash !== hovered?.trash) {
            hovered = place;
            spring.restart();
        }
        if (place !== null && !place.trash && !(answerKey(place.folder, copy) in answers))
            ask(place.folder, copy, null);
        drag.accepted = action !== Drops.NONE;
        if (drag.accepted)
            drag.action = Drops.dragAction(action);
    }

    function leave() {
        hovered = null;
        spring.stop();
    }

    // A drag came in: `drag` is the DragEvent, or anything with its x, y,
    // urls, proposedAction, accepted and action.
    function enter(drag: var) {
        sources = Drops.localPaths(drag.urls);
        answers = {};
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

    // The drag was let go: `drop` is the DragEvent, or anything with its x,
    // y, proposedAction, accepted and accept(). The place is looked up
    // again: a folder opened by resting on it may have put another there.
    function release(drop: var) {
        const place = folderAt(drop.x, drop.y);
        const copy = drop.proposedAction === Qt.CopyAction;
        const action = what(place, copy, answers);
        const sources = target.sources;
        leave();
        if (place === null || (action === Drops.NONE && answerKey(place.folder, copy) in answers)) {
            drop.accepted = false;
            return;
        }
        drop.accept(action === Drops.NONE ? drop.proposedAction : Drops.dragAction(action));
        if (action !== Drops.NONE) {
            dropRequested(action, sources, place.folder);
            return;
        }
        // Dropped before novad answered for here: ask, then do it.
        ask(place.folder, copy, settled => {
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
            if (!target.hovered?.spring || target.action === Drops.NONE)
                return;
            const folder = target.hovered.folder;
            // What's under the pointer changes as the folder opens: the
            // next move or the drop looks again.
            target.hovered = null;
            target.springOpened(folder);
        }
    }
}
