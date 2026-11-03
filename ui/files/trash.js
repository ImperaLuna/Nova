.pragma library
.import "../daemon/protocol.js" as Protocol
.import "../text/bytes.js" as Bytes
.import "listing.js" as Listing
.import "paths.js" as Paths
.import "transfers.js" as Transfers

// Nova Files' trash as its window shows it, kept apart so ui/tests can
// check it: a trashed item as a row, where it came from, the command row's
// Restore, the footer, and the words of Windows' "Delete file" and "Delete
// multiple items" dialogs that ask before anything is deleted for good.

// The trash's name, in the sidebar, the address bar and the window title.
function name() {
    return qsTr("Trash");
}

// The address bar's one part while a window shows the trash.
function crumbs() {
    return [{
            name: name(),
            path: Protocol.FILES_TRASH_PLACE,
            hidden: false
        }];
}

// Whether a window at `folder` shows the trash.
function isTrash(folder) {
    return folder === Protocol.FILES_TRASH_PLACE;
}

// The absolute paths of `rows` (the file list's) in `folder`.
function paths(folder, rows) {
    return rows.map(row => Paths.child(folder, row.name));
}

// What a row holds: a trashed item ({id, folder, deleted_ms, entry}) as
// the file list's rows hold an entry, plus its id (`trashId`: a delegate
// can't have a property named id), the folder it came from and when it was
// trashed (-1 when the trash doesn't say).
function row(item) {
    const shown = Listing.row(item.entry);
    shown.trashId = item.id;
    shown.folder = item.folder;
    shown.deleted = item.deleted_ms ?? -1;
    return shown;
}

// The command row's Restore: the picked items, or everything.
function restoreLabel(selected) {
    return selected > 0 ? qsTr("Restore") : qsTr("Restore all");
}

// The footer: how many items, or how many of them are picked.
function footer(selected, total) {
    return selected > 0 ? qsTr("%1 of %2 selected").arg(selected).arg(total) : Listing.items(total);
}

// The question before rows (the file list's or the trash's) are deleted
// for good: {title, question, item}, where `item` is the one row shown
// under the question, or null for several.
function confirm(rows) {
    if (rows.length !== 1)
        return {
            title: qsTr("Delete multiple items"),
            question: qsTr("Are you sure you want to permanently delete these %1 items?").arg(rows.length),
            item: null
        };
    const item = rows[0];
    return {
        title: item.dir ? qsTr("Delete folder") : qsTr("Delete file"),
        question: item.dir ? qsTr("Are you sure you want to permanently delete this folder?") : qsTr("Are you sure you want to permanently delete this file?"),
        item: item
    };
}

// The lines under a confirmed item's name, as Windows shows them: its
// type, its size (not for a folder) and when it was last changed.
function details(item) {
    const lines = [qsTr("Type: %1").arg(Listing.kind(item))];
    if (!item.dir && item.size >= 0)
        lines.push(qsTr("Size: %1").arg(Bytes.bytes(item.size)));
    if (item.modified >= 0)
        lines.push(qsTr("Date modified: %1").arg(Qt.formatDateTime(new Date(item.modified), Transfers.MODIFIED_FORMAT)));
    return lines;
}

// Room in a request for all but the list: its id, command and the rest.
var REQUEST_ROOM = 1024;

// `items` (strings) split into lists that each fit in a request of at most
// `limit` bytes, counting each as JSON at its longest in UTF-8 (3 bytes for
// each UTF-16 unit). Never empty: no items still make one empty list.
function batches(items, limit) {
    const most = limit - REQUEST_ROOM;
    const lists = [[]];
    let size = 0;
    for (const item of items) {
        const bytes = 3 * JSON.stringify(item).length + 1;
        if (size + bytes > most && lists[lists.length - 1].length > 0) {
            lists.push([]);
            size = 0;
        }
        lists[lists.length - 1].push(item);
        size += bytes;
    }
    return lists;
}
