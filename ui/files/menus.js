.pragma library

.import "commands.js" as Commands
.import "shortcuts.js" as Shortcuts
.import "../daemon/protocol.js" as Protocol
.import "../theme/icons.js" as Icons

// Nova Files' menus as FilesMenu shows them: right-click on items (COSMIC
// Files' folder menu, with Copy beside Cut and Move to trash for Delete),
// on the empty list, on a drive, and the command row's New and Sort. Each
// item calls `act(command, rows, extra)` with a Commands name; only what
// Nova Files can do yet is listed.

function _item(label, icon, command, act, rows, extra) {
    return {
        label: label,
        icon: icon,
        shortcut: Shortcuts.shown(command),
        act: () => act(command, rows, extra)
    };
}

var SEPARATOR = {
    separator: true
};

// The menu for `rows` (plain copies, at least one), in an archive when
// `inArchive` (where nothing can be changed). `apps()` gives the apps that
// can open a single file ({id, name}) and `defaultApp` its default, for
// Open with. A single archive picked gets Extract all… and Extract here
// after the ways to open it, and items outside an archive get Compress to
// ZIP file before Copy path, as in Explorer.
function forItems(rows, inArchive, apps, defaultApp, act) {
    const single = rows.length === 1;
    const folders = rows.every(row => row.dir);
    const menu = [_item(qsTr("Open"), Icons.OPEN, Commands.OPEN, act, rows)];
    if (single && !rows[0].dir)
        menu.push({
            label: qsTr("Open with…"),
            icon: "",
            items: () => apps().map(app => ({
                        label: app.name,
                        icon: "",
                        checked: app.id === defaultApp(),
                        act: () => act(Commands.OPEN_WITH, rows, app.id)
                    }))
        });
    if (folders)
        menu.push(_item(qsTr("Open in new tab"), "", Commands.NEW_TAB, act, rows), _item(qsTr("Open in new window"), "", Commands.NEW_WINDOW, act, rows));
    if (single && rows[0].archive && !inArchive)
        menu.push(_item(qsTr("Extract all…"), Icons.EXTRACT, Commands.EXTRACT_ALL, act, rows), _item(qsTr("Extract here"), "", Commands.EXTRACT_HERE, act, rows));
    menu.push(SEPARATOR);
    if (single && !inArchive)
        menu.push(_item(qsTr("Rename…"), Icons.RENAME, Commands.RENAME, act, rows));
    if (!inArchive)
        menu.push(_item(qsTr("Cut"), Icons.CUT, Commands.CUT, act, rows));
    menu.push(_item(qsTr("Copy"), Icons.COPY, Commands.COPY, act, rows));
    if (!inArchive)
        menu.push(_item(qsTr("Compress to ZIP file"), Icons.ARCHIVE, Commands.COMPRESS, act, rows));
    menu.push(_item(qsTr("Copy path"), "", Commands.COPY_PATH, act, rows));
    if (single)
        menu.push(SEPARATOR, _item(qsTr("Show details…"), Icons.INFO, Commands.DETAILS, act, rows));
    if (!inArchive)
        menu.push(SEPARATOR, _item(qsTr("Move to trash"), Icons.TRASH, Commands.TRASH, act, rows));
    menu.push(SEPARATOR, _properties(act, rows));
    return menu;
}

// What undoing or redoing a step (a Protocol.FILE_STEP_*) is called, as
// in Explorer's "Undo Rename".
function _undoLabel(redo, step) {
    const steps = {};
    steps[Protocol.FILE_STEP_COPY] = redo ? qsTr("Redo copy") : qsTr("Undo copy");
    steps[Protocol.FILE_STEP_MOVE] = redo ? qsTr("Redo move") : qsTr("Undo move");
    steps[Protocol.FILE_STEP_TRASH] = redo ? qsTr("Redo move to trash") : qsTr("Undo move to trash");
    steps[Protocol.FILE_STEP_RENAME] = redo ? qsTr("Redo rename") : qsTr("Undo rename");
    steps[Protocol.FILE_STEP_NEW_FOLDER] = redo ? qsTr("Redo new folder") : qsTr("Undo new folder");
    return steps[step] ?? (redo ? qsTr("Redo") : qsTr("Undo"));
}

// The menu for the folder itself, from the empty list. `history` is what
// can be undone and redone ({undo, redo}, FilesService.history); each is
// listed only when there's one, after Paste as in Explorer.
function forFolder(canPaste, hidden, history, act) {
    const paste = _item(qsTr("Paste"), Icons.PASTE, Commands.PASTE, act, []);
    paste.enabled = canPaste;
    const showHidden = _item(qsTr("Show hidden files"), "", Commands.HIDDEN, act, []);
    showHidden.checked = hidden;
    const menu = [_item(qsTr("New folder"), Icons.FOLDER, Commands.NEW_FOLDER, act, []), paste];
    if (history.undo)
        menu.push(_item(_undoLabel(false, history.undo), Icons.UNDO, Commands.UNDO, act, []));
    if (history.redo)
        menu.push(_item(_undoLabel(true, history.redo), Icons.REDO, Commands.REDO, act, []));
    return menu.concat([SEPARATOR, _item(qsTr("Select all"), "", Commands.SELECT_ALL, act, []), showHidden, SEPARATOR, _item(qsTr("Show details…"), Icons.INFO, Commands.DETAILS, act, []), _properties(act, [])]);
}

// The command row's New.
function forNew(act) {
    return [_item(qsTr("Folder"), Icons.FOLDER, Commands.NEW_FOLDER, act, [])];
}

// A drive's menu: Eject only for one that can be unplugged, Properties
// (how full it is) only while it's mounted.
function forDrive(drive, act) {
    const menu = [_item(qsTr("Open in new tab"), "", Commands.NEW_TAB, act, [], drive), _item(qsTr("Open in new window"), "", Commands.NEW_WINDOW, act, [], drive), _item(qsTr("Show details…"), Icons.INFO, Commands.DETAILS, act, [], drive)];
    if (drive.removable)
        menu.push(SEPARATOR, _item(qsTr("Eject"), Icons.EJECT, Commands.EJECT, act, [], drive));
    if (drive.mount !== undefined)
        menu.push(SEPARATOR, _properties(act, [], drive));
    return menu;
}

// Properties, last in a menu as in Explorer.
function _properties(act, rows, drive) {
    return _item(qsTr("Properties"), "", Commands.PROPERTIES, act, rows, drive);
}

// The command row's Sort: the four columns, then the direction, the
// current ones checked. Each picks {key, descending}.
function forSort(sort, act) {
    const keys = [[qsTr("Name"), Protocol.FILE_SORT_NAME], [qsTr("Modified"), Protocol.FILE_SORT_MODIFIED], [qsTr("Type"), Protocol.FILE_SORT_KIND], [qsTr("Size"), Protocol.FILE_SORT_SIZE]];
    const pick = (key, descending) => ({
            key: key,
            descending: descending
        });
    const menu = keys.map(([label, key]) => ({
                label: label,
                icon: "",
                checked: sort.key === key,
                act: () => act(Commands.SORT, [], pick(key, sort.descending))
            }));
    menu.push(SEPARATOR, {
        label: qsTr("Ascending"),
        icon: "",
        checked: !sort.descending,
        act: () => act(Commands.SORT, [], pick(sort.key, false))
    }, {
        label: qsTr("Descending"),
        icon: "",
        checked: sort.descending,
        act: () => act(Commands.SORT, [], pick(sort.key, true))
    });
    return menu;
}
