.pragma library
.import "../daemon/protocol.js" as Protocol
.import "../text/bytes.js" as Bytes
.import "../theme/icons.js" as Icons

// Nova Files' file list logic, kept apart so ui/tests can check it: how an
// entry of novad's folder view reads in the Details columns (icon, Modified,
// Type, Size), the order a column header click asks for, what a click or a
// key selects, what opening the selection does, and how much of a name the
// rename field picks. Rows are novad's entries flattened for a ListModel
// (`row`).

// What a row holds: novad's entry with its optional size and modified time
// as -1 when it has none, since a ListModel's rows all need every role.
function row(entry) {
    return {
        name: entry.name,
        dir: entry.dir,
        link: entry.link,
        hidden: entry.hidden,
        mime: entry.mime,
        size: entry.size ?? -1,
        modified: entry.modified_ms ?? -1
    };
}

// A row of the list's model as a plain object, which stays as it is when
// the model changes under it: what a dialog or a late answer holds on to.
function copy(row) {
    return {
        name: row.name,
        dir: row.dir,
        link: row.link,
        hidden: row.hidden,
        mime: row.mime,
        size: row.size,
        modified: row.modified
    };
}

// The line icon for a row: a folder, a picture, a video, music, a document
// with text, or a plain file.
function icon(row) {
    if (row.dir)
        return Icons.FOLDER;
    const group = row.mime.slice(0, row.mime.indexOf("/"));
    if (group === "image")
        return Icons.PICTURE;
    if (group === "video")
        return Icons.VIDEO;
    if (group === "audio")
        return Icons.MUSIC;
    if (group === "text" || row.mime === "application/pdf")
        return Icons.DOCUMENT;
    return Icons.FILE;
}

// The Type column: "Folder", or the MIME subtype without its vendor and
// experimental prefixes or its "+xml" kind of suffix, short ones in
// capitals ("PDF", "PNG"), longer ones capitalized ("Markdown"). Text and
// unknown data read "Text" and "File".
function kind(row) {
    if (row.dir)
        return qsTr("Folder");
    if (row.mime === "text/plain")
        return qsTr("Text");
    let subtype = row.mime.slice(row.mime.indexOf("/") + 1);
    subtype = subtype.replace(/^(x-|vnd\.)+/, "").replace(/\+.*$/, "");
    if (subtype === "" || subtype === "octet-stream")
        return qsTr("File");
    if (subtype.length <= SHORT_KIND)
        return subtype.toUpperCase();
    return subtype.charAt(0).toUpperCase() + subtype.slice(1);
}

// Subtypes this short are abbreviations (PDF, JPEG), read in capitals.
var SHORT_KIND = 4;

// The Modified column for `ms` (since 1970) as seen at `now`: "Today
// 16:12", "1 Nov" this year, "13 Jul 2025" before; empty when novad
// didn't say.
function modified(ms, now) {
    if (ms < 0)
        return "";
    const date = new Date(ms);
    const today = new Date(now);
    if (date.toDateString() === today.toDateString())
        return qsTr("Today %1").arg(Qt.formatTime(date, "HH:mm"));
    if (date.getFullYear() === today.getFullYear())
        return Qt.formatDate(date, "d MMM");
    return Qt.formatDate(date, "d MMM yyyy");
}

// The Size column: empty for a folder.
function size(row) {
    return row.dir || row.size < 0 ? "" : Bytes.bytes(row.size);
}

// The footer's count.
function items(count) {
    return count === 1 ? qsTr("1 item") : qsTr("%1 items").arg(count);
}

// The order after a click on the header of column `key` (a
// Protocol.FILE_SORT_*) while sorted by `sort` ({key, descending}): the
// same column turns the order round, another starts it ascending.
function sorted(sort, key) {
    return {
        key: key,
        descending: sort.key === key ? !sort.descending : false
    };
}

// Where row `index` is after one of a files_changed event's changes, so a
// remembered row (the Shift anchor) stays on the same entry. A removed
// row's place passes to the one after it.
function shifted(index, change) {
    if (index < 0)
        return index;
    if (change.op === Protocol.FOLDER_INSERT && change.index <= index)
        return index + 1;
    if (change.op === Protocol.FOLDER_REMOVE && change.index < index)
        return index - 1;
    return index;
}

// What a click on row `index` selects with Ctrl and Shift as held, from
// the row the last plain or Ctrl click was on (`anchor`, -1 for none):
// {clear, from, to, toggle, anchor}. Rows from..to are selected after
// clearing the rest when `clear`, or flipped when `toggle`. Shift selects
// the rows from the anchor (adding to the selection with Ctrl too), Ctrl
// flips one row, a plain click selects only that row.
function pick(anchor, index, ctrl, shift) {
    if (shift && anchor >= 0) {
        return {
            clear: !ctrl,
            from: Math.min(anchor, index),
            to: Math.max(anchor, index),
            toggle: false,
            anchor: anchor
        };
    }
    return {
        clear: !ctrl,
        from: index,
        to: index,
        toggle: ctrl,
        anchor: index
    };
}

// The keys that move through the rows.
var MOVES = [Qt.Key_Up, Qt.Key_Down, Qt.Key_PageUp, Qt.Key_PageDown, Qt.Key_Home, Qt.Key_End];

// The row a navigation key moves to from `current` among `count` rows,
// `page` rows to a screen; `current` itself for any other key.
function moved(key, current, count, page) {
    const last = count - 1;
    const from = Math.max(current, 0);
    switch (key) {
    case Qt.Key_Up:
        return current < 0 ? 0 : Math.max(from - 1, 0);
    case Qt.Key_Down:
        return current < 0 ? 0 : Math.min(from + 1, last);
    case Qt.Key_PageUp:
        return Math.max(from - page, 0);
    case Qt.Key_PageDown:
        return Math.min(from + page, last);
    case Qt.Key_Home:
        return 0;
    case Qt.Key_End:
        return last;
    }
    return current;
}

// What opening the selected rows does: {folder, files}. One selected
// folder is gone into; otherwise every selected file opens in its app,
// and folders among them stay put.
function opening(rows) {
    if (rows.length === 1 && rows[0].dir)
        return {
            folder: rows[0].name,
            files: []
        };
    return {
        folder: "",
        files: rows.filter(row => !row.dir).map(row => row.name)
    };
}

// How much of a name the rename field picks at first: a file's name up to
// its extension, as in Explorer; a folder's, or a dot file's, whole.
function stemLength(name, dir) {
    const dot = name.lastIndexOf(".");
    return dir || dot <= 0 ? name.length : dot;
}
