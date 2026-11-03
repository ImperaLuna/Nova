.pragma library

// What Nova Files does from its menus, its buttons and its keys, by name.
// Each key that runs one is in shortcuts.js, which menus read their
// shortcut from too.

var OPEN = "open";
var OPEN_WITH = "open_with";
// On folders picked (Shift+Enter, a menu), or on the window's own folder
// when none are (Ctrl+N).
var NEW_WINDOW = "new_window";
// Tabs: on folders picked (a menu) or a drive, each in a tab behind; with
// none, a new tab at Home in front (Ctrl+T, +). Closing the last tab
// closes the window.
var NEW_TAB = "new_tab";
var CLOSE_TAB = "close_tab";
var NEXT_TAB = "next_tab";
var PREVIOUS_TAB = "previous_tab";
var RENAME = "rename";
var CUT = "cut";
var COPY = "copy";
var COPY_PATH = "copy_path";
var PASTE = "paste";
var NEW_FOLDER = "new_folder";
var DETAILS = "details";
// Shows or hides the preview pane, which takes the details pane's place.
var PREVIEW = "preview";
// The Properties dialog: on the items picked, the folder, or a drive.
var PROPERTIES = "properties";
var TRASH = "trash";
var DELETE_FOR_GOOD = "delete_for_good";
var SELECT_ALL = "select_all";
var HIDDEN = "hidden";
var REFRESH = "refresh";
// The last step taken in any window, and the last one undone.
var UNDO = "undo";
var REDO = "redo";

// Moving about.
var BACK = "back";
var FORWARD = "forward";
var UP = "up";
var ADDRESS = "address";
var SEARCH = "search";
// The arrows, Page Up/Down, Home and End through a list's rows.
var MOVE = "move";

// Archives: "Extract all…" into a folder named after one (the one picked,
// or the one the window is in), "Extract here" beside it, and "Compress
// to ZIP file" for the items picked.
var EXTRACT_ALL = "extract_all";
var EXTRACT_HERE = "extract_here";
var COMPRESS = "compress";

// From the drive menu and the command row's Sort.
var EJECT = "eject";
var SORT = "sort";
