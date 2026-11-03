.pragma library

// What Nova Files' file list does from its menus and its keys, by name,
// with each one's shortcut written once: the menus show it and a key
// press is matched against it. Shortcuts are COSMIC Files' and Explorer's.

var OPEN = "open";
var OPEN_WITH = "open_with";
var NEW_WINDOW = "new_window";
var RENAME = "rename";
var CUT = "cut";
var COPY = "copy";
var COPY_PATH = "copy_path";
var PASTE = "paste";
var NEW_FOLDER = "new_folder";
var DETAILS = "details";
var TRASH = "trash";
var SELECT_ALL = "select_all";
var HIDDEN = "hidden";

var SHORTCUTS = {
    [OPEN]: "Enter",
    [NEW_WINDOW]: "Shift+Enter",
    [RENAME]: "F2",
    [CUT]: "Ctrl+X",
    [COPY]: "Ctrl+C",
    [COPY_PATH]: "Ctrl+Shift+C",
    [PASTE]: "Ctrl+V",
    [NEW_FOLDER]: "Ctrl+Shift+N",
    [DETAILS]: "Ctrl+Space",
    [TRASH]: "Delete",
    [SELECT_ALL]: "Ctrl+A",
    [HIDDEN]: "Ctrl+H"
};

// Key names as shortcuts spell them.
var KEYS = {
    [Qt.Key_Return]: "Enter",
    [Qt.Key_Enter]: "Enter",
    [Qt.Key_F2]: "F2",
    [Qt.Key_Space]: "Space",
    [Qt.Key_Delete]: "Delete",
    [Qt.Key_A]: "A",
    [Qt.Key_C]: "C",
    [Qt.Key_H]: "H",
    [Qt.Key_N]: "N",
    [Qt.Key_V]: "V",
    [Qt.Key_X]: "X"
};

// The shortcut as a menu shows it: "Ctrl + X".
function shown(command) {
    return (SHORTCUTS[command] ?? "").split("+").join(" + ");
}

// The command a key press with `modifiers` asks for, or "".
function forKey(key, modifiers) {
    const name = KEYS[key];
    if (name === undefined || (modifiers & (Qt.AltModifier | Qt.MetaModifier)) !== 0)
        return "";
    const ctrl = (modifiers & Qt.ControlModifier) !== 0 ? "Ctrl+" : "";
    const shift = (modifiers & Qt.ShiftModifier) !== 0 ? "Shift+" : "";
    const pressed = ctrl + shift + name;
    return Object.keys(SHORTCUTS).find(command => SHORTCUTS[command] === pressed) ?? "";
}

// From the drive menu and the command row's Sort.
var EJECT = "eject";
var SORT = "sort";
