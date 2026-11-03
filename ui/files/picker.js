.pragma library
.import "../daemon/protocol.js" as Protocol
.import "paths.js" as Paths

// A Nova Files window in picker mode (an app's Open or Save dialog, novad's
// files_window `picker`), kept apart so ui/tests can check it: its title,
// its accept button's label, and what that button does with the selection
// and the typed name, as Windows' Open and Save As dialogs do.

// The window's title: the app's, or the mode's.
function title(picker) {
    if (picker.title)
        return picker.title;
    if (picker.mode === Protocol.PICK_OPEN)
        return picker.folders ? qsTr("Select Folder") : qsTr("Open");
    return qsTr("Save As");
}

// The accept button's label: the app's (its mnemonic underscores dropped,
// "__" kept as one), or the mode's.
function acceptLabel(picker) {
    if (picker.accept)
        return picker.accept.replace(/_(.)/g, "$1");
    if (picker.mode === Protocol.PICK_OPEN)
        return picker.folders ? qsTr("Select Folder") : qsTr("Open");
    return qsTr("Save");
}

// Whether the window picks a folder: Select Folder, or saving several files.
function picksFolder(picker) {
    return picker.mode === Protocol.PICK_SAVE_FILES || (picker.mode === Protocol.PICK_OPEN && picker.folders);
}

// The name field's text selected at first: the name without its type, so
// typing keeps ".png".
function stemLength(name) {
    const dot = name.lastIndexOf(".");
    return dot > 0 ? dot : name.length;
}

// The filter that lists only folders, for a picker that picks one: a
// filter matching no file.
var ONLY_FOLDERS = {
    name: "",
    globs: [],
    mimes: []
};

// The filter the folder view applies: the picker's filter at `index`, or
// null for every file.
function filter(picker, index) {
    return index >= 0 && index < (picker.filters ?? []).length ? picker.filters[index] : null;
}

// What the accept button does in `folder` with the `selected` rows ({name,
// dir}) and, for a save, the typed `name` and the listed row of that name
// (`typedRow`, or null):
// - {action: "pick", paths}: answer the app with these absolute paths;
// - {action: "enter", folder}: go into that folder first;
// - {action: "none"}: nothing to pick yet, so the button is off.
function choice(picker, folder, selected, name, typedRow) {
    const none = {
        action: "none"
    };
    const pick = paths => ({
            action: "pick",
            paths: paths
        });
    const enter = path => ({
            action: "enter",
            folder: path
        });
    const folders = selected.filter(row => row.dir);
    const files = selected.filter(row => !row.dir);
    if (picker.mode === Protocol.PICK_SAVE) {
        const typed = name.trim();
        if (typed === "" || typed === "." || typed === ".." || typed.includes("/"))
            return none;
        if (typedRow?.dir)
            return enter(Paths.child(folder, typed));
        return pick([Paths.child(folder, typed)]);
    }
    if (picksFolder(picker)) {
        if (folders.length > 0 && files.length === 0)
            return pick(folders.map(row => Paths.child(folder, row.name)));
        return pick([folder]);
    }
    if (files.length > 0)
        return pick(files.map(row => Paths.child(folder, row.name)));
    if (folders.length === 1)
        return enter(Paths.child(folder, folders[0].name));
    return none;
}

// The Save As dialog's question when the file named `name` (as shown) is
// there already, in ConfirmDialog's {title, question, item}.
function replacing(name) {
    return {
        title: qsTr("Confirm Save As"),
        question: qsTr("%1 already exists.\nDo you want to replace it?").arg(name),
        item: null
    };
}
