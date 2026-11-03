.pragma library
.import "commands.js" as Commands

// Every key Nova Files answers, after Windows Explorer, in one table: the
// window (FilesWindow), the file list (FileList) and the trash's list
// (TrashList) each read their part of it, so ui/tests can check them all
// together, and menus show a command's key from it (shown()). An entry is
// {action, keys, where}: `action` a Commands name, `keys` Qt's portable key
// sequence names ("Ctrl+Shift+N", "PgUp"), `where` the places it works.
//
// The rules that keep it free of surprises:
// - A WINDOW key works anywhere in the window, also while a text field has
//   the keyboard, so it is never a key a text field edits with (TEXT_KEYS).
// - LIST and TRASH keys work only while that list has the keyboard, so a
//   text field (the address bar, search) keeps every editing key.
// - No key means two things in the same place.
// - While a dialog asks, only the WINDOW keys marked `asking` (opening and
//   closing windows) work, so nothing moves under the question.
// Typing a name in a list (type-to-select) isn't a key here: it is any
// printable text without Ctrl or Alt (typed()).

var WINDOW = "window";
var LIST = "list";
var TRASH = "trash";

var TABLE = [
    {
        action: Commands.BACK,
        keys: ["Alt+Left"],
        where: [WINDOW]
    },
    // Explorer's Backspace views the previous folder, like Alt+Left.
    {
        action: Commands.BACK,
        keys: ["Backspace"],
        where: [LIST, TRASH]
    },
    {
        action: Commands.FORWARD,
        keys: ["Alt+Right"],
        where: [WINDOW]
    },
    {
        action: Commands.UP,
        keys: ["Alt+Up"],
        where: [WINDOW]
    },
    {
        action: Commands.ADDRESS,
        keys: ["Ctrl+L", "Alt+D"],
        where: [WINDOW]
    },
    {
        action: Commands.SEARCH,
        keys: ["Ctrl+F", "F3"],
        where: [WINDOW]
    },
    {
        action: Commands.REFRESH,
        keys: ["F5"],
        where: [WINDOW]
    },
    // The picked folders in new windows; Ctrl+N (the window's) the folder
    // shown.
    {
        action: Commands.NEW_WINDOW,
        keys: ["Shift+Return", "Shift+Enter"],
        where: [LIST]
    },
    {
        action: Commands.NEW_WINDOW,
        asking: true,
        keys: ["Ctrl+N"],
        where: [WINDOW]
    },
    {
        action: Commands.CLOSE_WINDOW,
        asking: true,
        keys: ["Ctrl+W"],
        where: [WINDOW]
    },
    {
        action: Commands.NEW_FOLDER,
        keys: ["Ctrl+Shift+N"],
        where: [LIST]
    },
    {
        action: Commands.RENAME,
        keys: ["F2"],
        where: [LIST]
    },
    {
        action: Commands.CUT,
        keys: ["Ctrl+X"],
        where: [LIST]
    },
    {
        action: Commands.COPY,
        keys: ["Ctrl+C"],
        where: [LIST]
    },
    // Explorer's Copy as path.
    {
        action: Commands.COPY_PATH,
        keys: ["Ctrl+Shift+C"],
        where: [LIST]
    },
    {
        action: Commands.PASTE,
        keys: ["Ctrl+V"],
        where: [LIST]
    },
    // Explorer's Details pane.
    {
        action: Commands.DETAILS,
        keys: ["Alt+Shift+P"],
        where: [LIST]
    },
    {
        action: Commands.HIDDEN,
        keys: ["Ctrl+H"],
        where: [LIST]
    },
    {
        action: Commands.SELECT_ALL,
        keys: ["Ctrl+A"],
        where: [LIST, TRASH]
    },
    {
        action: Commands.OPEN,
        keys: ["Return", "Enter"],
        where: [LIST]
    },
    {
        action: Commands.TRASH,
        keys: ["Del"],
        where: [LIST]
    },
    // In the trash, Delete already deletes for good (Explorer's Recycle
    // Bin does the same), so Shift+Delete does too.
    {
        action: Commands.DELETE_FOR_GOOD,
        keys: ["Shift+Del"],
        where: [LIST]
    },
    {
        action: Commands.DELETE_FOR_GOOD,
        keys: ["Del", "Shift+Del"],
        where: [TRASH]
    },
    // Undo and redo the last step taken in any window. A text field keeps
    // them for its own text, so they're the lists'.
    {
        action: Commands.UNDO,
        keys: ["Ctrl+Z"],
        where: [LIST, TRASH]
    },
    {
        action: Commands.REDO,
        keys: ["Ctrl+Y", "Ctrl+Shift+Z"],
        where: [LIST, TRASH]
    },
    // The moves: with Shift each extends the selection from its anchor.
    {
        action: Commands.MOVE,
        keys: ["Up", "Down", "PgUp", "PgDown", "Home", "End", "Shift+Up", "Shift+Down", "Shift+PgUp", "Shift+PgDown", "Shift+Home", "Shift+End"],
        where: [LIST, TRASH]
    }
];

// The keys a text field edits with: never a WINDOW key.
var TEXT_KEYS = ["Backspace", "Del", "Left", "Right", "Up", "Down", "Home", "End", "Ctrl+A", "Ctrl+C", "Ctrl+X", "Ctrl+V", "Ctrl+Z", "Ctrl+Y", "Ctrl+Shift+Z", "Ctrl+Left", "Ctrl+Right", "Ctrl+Backspace", "Ctrl+Del", "Shift+Left", "Shift+Right", "Shift+Home", "Shift+End", "Shift+Del", "Return", "Enter", "Esc"];

// The entries that work in `where`.
function entries(where) {
    return TABLE.filter(entry => entry.where.includes(where));
}

// How menus spell the portable names that read differently.
var SHOWN_NAMES = {
    Return: "Enter",
    Del: "Delete"
};

// The key a menu shows for `command`: its first in a list, else the
// window's ("Ctrl + Shift + C"), or "" for none.
function shown(command) {
    const entry = entries(LIST).find(candidate => candidate.action === command) ?? entries(WINDOW).find(candidate => candidate.action === command);
    if (!entry)
        return "";
    return entry.keys[0].split("+").map(part => SHOWN_NAMES[part] ?? part).join(" + ");
}

// The names of keys that aren't a letter, a digit or F1..F35.
var NAMES = {};
NAMES[Qt.Key_Backspace] = "Backspace";
NAMES[Qt.Key_Delete] = "Del";
NAMES[Qt.Key_Return] = "Return";
NAMES[Qt.Key_Enter] = "Enter";
NAMES[Qt.Key_Escape] = "Esc";
NAMES[Qt.Key_Left] = "Left";
NAMES[Qt.Key_Right] = "Right";
NAMES[Qt.Key_Up] = "Up";
NAMES[Qt.Key_Down] = "Down";
NAMES[Qt.Key_PageUp] = "PgUp";
NAMES[Qt.Key_PageDown] = "PgDown";
NAMES[Qt.Key_Home] = "Home";
NAMES[Qt.Key_End] = "End";
NAMES[Qt.Key_Space] = "Space";
NAMES[Qt.Key_Tab] = "Tab";

// A key press as a key sequence name ("Ctrl+Shift+N"), or "" for a key
// the table can't name (a modifier on its own).
function sequence(key, modifiers) {
    let name = NAMES[key];
    if (name === undefined && ((key >= Qt.Key_A && key <= Qt.Key_Z) || (key >= Qt.Key_0 && key <= Qt.Key_9)))
        name = String.fromCharCode(key);
    if (name === undefined && key >= Qt.Key_F1 && key <= Qt.Key_F35)
        name = `F${key - Qt.Key_F1 + 1}`;
    if (name === undefined)
        return "";
    let held = "";
    if (modifiers & Qt.ControlModifier)
        held += "Ctrl+";
    if (modifiers & Qt.AltModifier)
        held += "Alt+";
    if (modifiers & Qt.ShiftModifier)
        held += "Shift+";
    // No entry holds Super, so a key with it held does nothing here.
    if (modifiers & Qt.MetaModifier)
        held += "Meta+";
    return held + name;
}

// The action key `key` with `modifiers` does in `where`, or "".
function action(where, key, modifiers) {
    const pressed = sequence(key, modifiers);
    if (pressed === "")
        return "";
    const entry = TABLE.find(candidate => candidate.where.includes(where) && candidate.keys.includes(pressed));
    return entry ? entry.action : "";
}

// Below this and at DELETE_CHARACTER a key's text is a control character
// (Backspace, Tab, Esc), not typing.
var FIRST_PRINTABLE = 0x20;
var DELETE_CHARACTER = 0x7f;

// The text a key press types into a list's name search, or "" when it
// isn't typing: Ctrl or Alt held, nothing printable, or a space before any
// letter (a space only continues a name).
function typed(text, modifiers, typing) {
    if (modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))
        return "";
    if (text.length === 0 || text.charCodeAt(0) < FIRST_PRINTABLE || text.charCodeAt(0) === DELETE_CHARACTER)
        return "";
    if (text === " " && typing === "")
        return "";
    return text;
}

// The row typing `typing` goes to among `names` (in the list's order),
// from row `current`, or -1 for none. As in Explorer, the same letter
// typed again steps to the next name starting with it; a longer name
// stays on the current row while that still matches. The search wraps
// round, ignoring case.
function find(names, current, typing) {
    if (names.length === 0 || typing === "")
        return -1;
    const wanted = typing.toLowerCase();
    const repeated = [...wanted].every(letter => letter === wanted[0]);
    const prefix = repeated ? wanted[0] : wanted;
    const from = Math.max(current, 0) + (repeated && current >= 0 ? 1 : 0);
    for (let step = 0; step < names.length; step++) {
        const index = (from + step) % names.length;
        if (names[index].toLowerCase().startsWith(prefix))
            return index;
    }
    return -1;
}
