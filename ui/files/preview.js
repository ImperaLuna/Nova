.pragma library
.import "../daemon/protocol.js" as Protocol

// Nova Files' preview pane logic, kept apart so ui/tests can check it:
// which file a selection previews, and which face the pane shows for it
// and novad's answer (files_preview).

// What the pane shows (`face`).
// Nothing is selected.
var EMPTY = "empty";
// Several are: how many.
var COUNT = "count";
// One file, until novad answers.
var WAITING = "waiting";
// A picture: the image itself, a PDF's page or a video's still.
var IMAGE = "image";
// The first lines of a text file.
var TEXT = "text";
// What it is, as its icon and name: a folder, or a file with nothing to
// show (or a picture Qt couldn't read).
var ITEM = "item";

// The row the pane previews: the one selected (`count`, with `row` the
// first), or null for none or several.
function subject(count, row) {
    return count === 1 && row ? row : null;
}

// Whether the pane asks novad about `row`: a file, not a folder.
function asks(row) {
    return row !== null && !row.dir;
}

// The pane's face for `count` selected, the previewed row `row`
// (`subject`), novad's answer `preview` for it ({kind, ...}, null until it
// answers, or false when it failed) and whether a picture couldn't be
// read (`broken`).
function face(count, row, preview, broken) {
    if (count === 0)
        return EMPTY;
    if (row === null)
        return COUNT;
    if (!asks(row))
        return ITEM;
    if (preview === null)
        return WAITING;
    if (preview === false || preview.kind === Protocol.PREVIEW_NONE)
        return ITEM;
    if (preview.kind === Protocol.PREVIEW_IMAGE)
        return broken ? ITEM : IMAGE;
    if (preview.kind === Protocol.PREVIEW_TEXT)
        return TEXT;
    return ITEM;
}

