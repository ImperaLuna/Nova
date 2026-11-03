pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import "../theme"
import "service"
import "paths.js" as Paths
import "tabs.js" as Tabs
import "drops.js" as Drops
import "trash.js" as Trash
import "search.js" as Search
import "commands.js" as Commands
import "properties.js" as Props
import "shortcuts.js" as Shortcuts
import "menus.js" as Menus
import "picker.js" as Picker
import "../daemon/protocol.js" as Protocol

// A Nova Files window: an app window the compositor tiles and rounds,
// showing `folder` with `select` selected. The tab row, the navigation
// row, the command row, then the sidebar beside the file list, with its
// copies and moves over them all; opening a folder in the list shows it in
// the same window.
// Inside an archive (a zip or tar, opened like a folder) nothing can be
// changed: its command row offers "Extract all" instead.
// At the trash, the trash's command row and list take the place of the
// folder's, and anything deleted for good is asked about first
// (ConfirmDialog). Right-click menus (menus.js: on items, on the empty
// list, on a drive, and the command row's New and Sort), the Properties
// dialog (Properties in a menu, Alt+Enter) and the dialog when a drive
// can't be ejected lie over it all. Show details (Alt+Shift+P) opens or
// closes the details pane on the right of the list, on the picked item or
// the folder; a drive's always opens it on the drive.
// Cut, copy and paste go through the system clipboard (FilesClipboard);
// pasting copies or moves into the folder shown, like dropping there.
// Files dropped on the list or the sidebar are copied, moved or trashed
// here, as novad answered for the drop (DropTarget).
// The preview pane (Alt+P, or the command row's button) takes the
// details pane's place at the list's right, loaded only while it shows.
// Each tab (FilesTab) has its own Navigator; the rows, the sidebar and the
// one file list follow the shown tab's, and the list's order and
// selection are kept with a tab as it goes to the back (the table's tab
// keys; a middle click on a folder, or "Open in new tab", opens it in a
// tab behind). Closing the last tab closes the window; a tab torn out of
// the row asks for a window of its own (`windowRequested`), carrying its
// history, order and selection.
// While something is typed in the search field, the search results take
// the file list's place (SearchResults); moving to another folder empties
// the field.
// `folder` follows the window's Navigator as it moves (Alt+Left,
// Alt+Right, Alt+Up and the mouse's back and forward buttons too). The
// window's keys are the table's WINDOW entries (shortcuts.js), and every
// command, from a key, a menu or a button, is done by act(); undo and redo
// (Ctrl+Z, Ctrl+Y, the folder's menu) are novad's, alike in every window. Its
// frame and its file list each go from glass to solid as config.toml's
// [files] says, live. Each draws its own background, so one never shows
// through the other.
// With a `picker` it's an app's Open or Save dialog: the picker bar sits
// under the list (PickerBar), the list shows the app's file types, opening
// files picks them, and the window closes once novad has the answer, or
// tells the app it was cancelled when closed any other way. A save over a
// file that's there asks first (ConfirmDialog).
FloatingWindow {
    id: window

    required property string folder
    property var select: []
    // novad's file picker ({id, mode, title, accept, multiple, folders,
    // name, filters, filter}), or null for a plain window.
    property var picker: null
    readonly property bool picking: picker !== null
    // The app has its answer (or stopped asking): closing tells it nothing.
    property bool answered: false
    // The picker bar, while it's a picker.
    readonly property PickerBar pickerBar: pickerLoader.item as PickerBar
    readonly property string typedName: pickerBar?.name ?? ""
    // What the picker bar's accept button does now (Picker.choice), from
    // the selection and the typed name, read again as either changes.
    readonly property var choice: picking && !inTrash ? Picker.choice(picker, folder, list.model.revision >= 0 && list.model.total >= 0 ? list.model.selectedRows() : [], typedName, list.model.total >= 0 ? list.model.rowNamed(typedName.trim()) : null) : null
    // A tab torn out of another window that this one starts as:
    // {history, view (from snapshot()), hidden}, or null.
    property var carried: null

    readonly property FilesTab firstTab: FilesTab {}
    property var tabs: [firstTab]
    property FilesTab tab: firstTab
    readonly property Navigator navigator: tab.navigator
    readonly property color frameColor: Tokens.mix(Tokens.glass, Tokens.filesSolidFrame, FilesService.frameSolid)

    readonly property bool inTrash: Trash.isTrash(folder)
    readonly property bool searching: search.active && !inTrash
    readonly property SearchModel search: SearchModel {
        service: FilesService
        query: navigation.query
        within: results.everywhere ? "" : window.folder
    }
    // The archive the folder is inside, or empty.
    readonly property string archive: list.model.archive
    // The archive "Extract all" extracts: the one the window is in, or the
    // one picked.
    readonly property string extractable: {
        if (archive !== "")
            return archive;
        const picked = list.model.revision >= 0 && list.model.selectedCount === 1 ? list.model.selectedRows()[0] : null;
        return picked?.archive ? Paths.child(folder, picked.name) : "";
    }
    readonly property string home: Quickshell.env("HOME") ?? ""
    // A dialog or menu has the window's attention: the confirm dialog, a
    // name clash, a menu, the Properties dialog or a drive's eject problem.
    readonly property bool asking: confirming !== null || transfers.asking !== null || menu.shown || propertiesModel.subject !== null || ejectProblem.visible
    // What the confirm dialog asks, and what a yes does: {asking, act}.
    property var confirming: null
    // Notices of what the trash couldn't do, oldest first: {row, error}.
    property var problems: []
    // The preview pane is out (Alt+P); it and the details pane take turns.
    property bool previewShown: false
    // The details pane is out, on `detailsPath` when it's a drive's, or else
    // on the one picked item, or the folder.
    property bool detailsShown: false
    property string detailsPath: ""
    readonly property string detailsOn: {
        if (detailsPath !== "")
            return detailsPath;
        void list.model.revision;
        const picked = list.model.selectedCount === 1 ? list.model.selectedRows() : [];
        return picked.length === 1 ? Paths.child(folder, picked[0].name) : folder;
    }

    // Runs `command` (a Commands name) from a menu, a button or a key, on
    // `rows` (plain copies of the list's) or, for a drive's menu, on the
    // drive `extra`; Open with's app and Sort's order come as `extra` too.
    function act(command: string, rows: var, extra: var) {
        const paths = rows.map(row => Paths.child(folder, row.name));
        switch (command) {
        case Commands.OPEN:
        case Commands.SELECT_ALL:
        case Commands.HIDDEN:
        case Commands.RENAME:
        case Commands.TRASH:
            list.run(command);
            break;
        case Commands.OPEN_WITH:
            FilesService.launch(paths[0], extra, null);
            break;
        case Commands.NEW_WINDOW:
            if (extra)
                window.onDrive(extra, mount => FilesService.openWindow(mount));
            else if (rows.length === 0)
                // From the trash, on the folder its tree is on, or home.
                FilesService.openWindow(inTrash ? (navigator.treeFolder || home) : folder);
            else
                for (const row of rows.filter(row => row.dir))
                    FilesService.openWindow(Paths.child(folder, row.name));
            break;
        case Commands.NEW_TAB:
            if (extra)
                window.onDrive(extra, mount => window.openTab(mount, false));
            else if (rows.length === 0)
                newTab();
            else
                for (const row of rows.filter(row => row.dir))
                    openTab(Paths.child(folder, row.name), false);
            break;
        case Commands.CLOSE_TAB:
            closeTab(tab);
            break;
        case Commands.NEXT_TAB:
            cycleTabs(1);
            break;
        case Commands.PREVIOUS_TAB:
            cycleTabs(-1);
            break;
        case Commands.BACK:
            navigator.step(-1);
            break;
        case Commands.FORWARD:
            navigator.step(1);
            break;
        case Commands.UP:
            navigator.up();
            break;
        case Commands.ADDRESS:
            navigation.editAddress();
            break;
        case Commands.SEARCH:
            // The trash isn't searched.
            if (!inTrash)
                navigation.focusSearch();
            break;
        case Commands.REFRESH:
            if (inTrash)
                trash.refresh();
            else
                list.refresh();
            break;
        case Commands.CUT:
        case Commands.COPY:
            if (paths.length > 0)
                FilesClipboard.put(paths, command === Commands.CUT);
            break;
        case Commands.COPY_PATH:
            if (paths.length > 0)
                FilesClipboard.putText(paths.join("\n"));
            break;
        case Commands.PASTE:
            window.paste();
            break;
        case Commands.EXTRACT_ALL:
        case Commands.EXTRACT_HERE:
            // The archive picked, or the one the window is in.
            if (paths.length === 1 || extractable !== "")
                window.extract(paths.length === 1 ? paths[0] : extractable, command === Commands.EXTRACT_HERE);
            break;
        case Commands.COMPRESS:
            window.compress(paths);
            break;
        case Commands.NEW_FOLDER:
            {
                // Named in place only if the window is still there.
                const asked = folder;
                FilesService.newFolder(asked, (error, name) => {
                    if (error)
                        window.notice(null, error);
                    else if (window.folder === asked)
                        list.rename(name);
                });
            }
            break;
        case Commands.DETAILS:
            // A drive's always shows; otherwise it toggles, like Alt+P.
            if (extra)
                window.onDrive(extra, mount => window.showDetails(mount));
            else if (detailsShown)
                detailsShown = false;
            else
                window.showDetails("");
            break;
        case Commands.PREVIEW:
            // The trash has no preview pane.
            if (!inTrash) {
                previewShown = !previewShown;
                detailsShown = false;
            }
            break;
        case Commands.PROPERTIES:
            window.showProperties(rows, extra);
            break;
        case Commands.EJECT:
            FilesService.eject(extra.id, outcome => {
                if (outcome === Protocol.EJECT_BUSY)
                    ejectProblem.show(extra.name);
            });
            break;
        case Commands.SORT:
            list.model.sort = extra;
            break;
        case Commands.UNDO:
        case Commands.REDO:
            transfers.undo(command === Commands.REDO, error => window.notice(null, error));
            break;
        default:
            console.warn(`nova: files: no command ${command}`);
        }
    }

    // Copies, or moves once cut, what the clipboard names into the folder.
    function paste() {
        const taken = FilesClipboard.take();
        if (taken.paths.length > 0)
            transfer(taken.mode, taken.paths, folder);
    }

    // Opens the details pane on `path` (a drive's folder), or on the
    // selection, as it changes, when empty.
    function showDetails(path: string) {
        detailsPath = path;
        detailsShown = true;
        previewShown = false;
    }

    // Calls `then(folder)` with where drive `drive` is mounted, mounting it
    // first when it isn't.
    function onDrive(drive: var, then: var) {
        if (drive.mount !== undefined)
            then(drive.mount);
        else
            FilesService.mount(drive.id, mount => {
                if (mount)
                    then(mount);
            });
    }

    // A menu of `items` (menus.js) at `at`, in `from`'s coordinates.
    function openMenu(items: var, from: Item, at: point) {
        const shown = from.mapToItem(menu, at.x, at.y);
        menu.open(items, Qt.point(shown.x, shown.y));
    }

    // A torn out tab wants a window like this one, starting as `carried`.
    signal windowRequested(string folder, var select, var carried)

    title: picking ? Picker.title(picker) : tab.title
    implicitWidth: Tokens.filesWindowWidth
    implicitHeight: Tokens.filesWindowHeight
    color: "transparent"

    // The compositor's blur behind the glass: the whole window, with the
    // corners the compositor rounds it to (niri and Hyprland don't round a
    // blur the window asks for). None once the frame and the list are both
    // solid, so solid costs nothing.
    readonly property bool glass: FilesService.frameSolid < 1 || FilesService.listSolid < 1
    Region {
        id: blur
        width: window.width
        height: window.height
        radius: Tokens.filesWindowRadius
    }
    function askForBlur() {
        BackgroundEffect.blurRegion = glass ? blur : null;
    }
    onGlassChanged: askForBlur()

    // Moves the rows (the file list's) in the folder shown to the trash, at
    // once: Delete.
    function moveToTrash(rows: var) {
        if (archive !== "")
            return;
        const paths = Trash.paths(folder, rows);
        FilesService.trash(paths, error => {
            if (error)
                window.notice(rows.length === 1 ? rows[0] : null, error);
        });
    }

    // Asks, then deletes the rows (the file list's) in the folder shown for
    // good: Shift+Delete.
    function deleteForGood(rows: var) {
        if (archive !== "")
            return;
        const paths = Trash.paths(folder, rows);
        confirm(rows, () => FilesService.deleteForGood(paths, error => {
                if (error)
                    window.notice(rows.length === 1 ? rows[0] : null, error);
            }));
    }

    // Puts the picked trashed items back, or all of them when none are
    // picked; each that can't go back gets a notice.
    function restore() {
        const rows = trash.selectedCount > 0 ? trash.selectedRows() : trash.allRows();
        for (const row of rows)
            FilesService.restore([row.trashId], error => {
                if (error)
                    window.notice(row, error);
            });
    }

    // Asks, then deletes the picked trashed items for good.
    function erase() {
        const rows = trash.selectedRows();
        if (rows.length > 0)
            confirm(rows, () => FilesService.eraseFromTrash(rows.map(row => row.trashId), error => {
                    if (error)
                        window.notice(rows.length === 1 ? rows[0] : null, error);
                }));
    }

    // Asks, then empties the trash.
    function empty() {
        const rows = trash.allRows();
        if (rows.length > 0)
            confirm(rows, () => FilesService.emptyTrash(error => {
                    if (error)
                        window.notice(null, error);
                }));
    }

    // Opens `path` (absolute, or from `~`) in a new tab at the end, shown
    // at once when `front`.
    function openTab(path: string, front: bool) {
        const opened = tabComponent.createObject(window) as FilesTab;
        opened.navigator.hidden = list.showHidden;
        opened.navigator.begin(path);
        tabs = tabs.concat([opened]);
        if (front)
            showTab(opened);
    }

    // A new tab at Home, as Explorer's opens on its Home: Ctrl+T and +.
    function newTab() {
        openTab(Quickshell.env("HOME") ?? "/", true);
    }

    // The shown tab's view: the file list's order and selection, and the
    // trash's selection.
    function snapshot(): var {
        return {
            folder: folder,
            list: list.tabView(),
            trash: trash.selectedIds()
        };
    }

    // Brings `next` to the front, keeping the shown tab's view with it.
    function showTab(next: FilesTab) {
        if (next === tab)
            return;
        tab.view = snapshot();
        front(next);
    }

    // Shows `next` as it was; its folder first, which clears the lists'
    // selections, then its view. A tab that moved on while at the back (a
    // move it asked for before) keeps its order but selects nothing.
    function front(next: FilesTab) {
        next.navigator.hidden = list.showHidden;
        tab = next;
        folder = next.navigator.folder;
        const kept = next.view?.folder === folder ? next.view : null;
        list.restoreTab(next.view?.list ?? null, kept?.list.selected ?? []);
        trash.selectIds(kept?.trash ?? []);
    }

    function cycleTabs(steps: int) {
        showTab(tabs[Tabs.cycled(tabs.indexOf(tab), tabs.length, steps)]);
    }

    function closeTab(closing: FilesTab) {
        const at = tabs.indexOf(closing);
        // The last tab takes the window with it. Hiding a window from QML
        // doesn't say `closed` (only the compositor closing it does), which
        // files.qml waits for.
        if (tabs.length === 1) {
            window.closed();
            return;
        }
        if (closing === tab)
            front(tabs[Tabs.neighbour(at, tabs.length)]);
        tabs = tabs.filter(other => other !== closing);
        // The first tab is part of the window and goes with it.
        if (closing !== firstTab)
            closing.destroy();
    }

    function moveTab(moving: FilesTab, to: int) {
        const from = tabs.indexOf(moving);
        if (from !== to)
            tabs = Tabs.reordered(tabs, from, to);
    }

    // Gives `torn` a window of its own and closes it here.
    function tearOut(torn: FilesTab) {
        const view = torn === tab ? snapshot() : torn.view;
        const place = torn.navigator.folder;
        windowRequested(place, (view?.list.selected ?? []).map(name => Paths.child(place, name)), {
            history: torn.navigator.history,
            view: view,
            hidden: list.showHidden
        });
        closeTab(torn);
    }

    function confirm(rows: var, act: var) {
        ask(Trash.confirm(rows), act);
    }

    // Asks `asking` ({title, question, item}) in the confirm dialog, and
    // calls `act` on a yes.
    function ask(asking: var, act: var) {
        confirming = {
            asking: asking,
            act: act
        };
        confirmDialog.forceActiveFocus();
    }

    // The picker bar's accept: picks, or goes into the folder first.
    function accept() {
        if (!choice || choice.action === "none")
            return;
        if (choice.action === "enter")
            navigator.visit(choice.folder, null);
        else
            pick(choice.paths, false);
    }

    // Answers the app with `paths`; a save over a file that's there asks
    // first, then picks again with `replace`.
    function pick(paths: var, replace: bool) {
        const filter = (picker.filters ?? []).length > 0 ? pickerBar.filterIndex : -1;
        FilesService.pick(picker.id, paths, filter, replace, error => {
            if (error === Protocol.FILES_PICK_EXISTS)
                window.ask(Picker.replacing(Paths.folderName(paths[0])), () => window.pick(paths, true));
            else if (error)
                window.notice(null, error);
            else
                window.finish();
        });
    }

    // Closes the window once the app has its answer.
    function finish() {
        answered = true;
        closed();
    }

    function answer(yes: bool) {
        const act = confirming?.act;
        confirming = null;
        if (yes)
            act?.();
        focusList();
    }

    // Shows the Properties of `rows` (the file list's) in the folder shown,
    // of the folder itself when there are none, or of `drive`.
    function showProperties(rows: var, drive: var) {
        propertiesModel.show(Props.subject(folder, rows, drive));
        propertiesDialog.forceActiveFocus();
    }

    function closeProperties() {
        propertiesModel.close();
        focusList();
    }

    function notice(row: var, error: string) {
        problems = problems.concat([
            {
                row: row,
                error: error
            }
        ]);
    }

    // Opens a search result (a Search.row): a folder is gone into, a file
    // opens in its default app.
    function openResult(row: var) {
        if (row.dir)
            navigator.visit(Search.path(row), ok => {
                if (ok)
                    list.forceActiveFocus();
            });
        else
            FilesService.launch(Search.path(row), "", null);
    }

    // Shows a search result in its folder, selected.
    function showResult(row: var) {
        navigator.visit(row.folder, ok => {
            if (!ok)
                return;
            navigation.clearSearch();
            list.reveal(row.name);
        });
    }

    Component.onCompleted: {
        askForBlur();
        if (carried) {
            list.showHidden = carried.hidden;
            navigator.hidden = carried.hidden;
            navigator.resume(carried.history);
            if (carried.view) {
                list.model.sort = carried.view.list.sort;
                trash.selectIds(carried.view.trash);
            }
        } else {
            navigator.begin(folder);
        }
        if (pickerBar?.naming)
            pickerBar.focusName();
    }
    onFolderChanged: {
        navigation.clearSearch();
        detailsPath = "";
    }
    onClosed: {
        if (picking && !answered)
            FilesService.cancelPick(picker.id);
    }

    Connections {
        target: FilesService
        enabled: window.picking
        function onPickerClosed(id: int) {
            if (id === window.picker.id)
                window.finish();
        }
    }
    // Saving: a file picked in the list is the name to save as.
    Connections {
        target: list.model
        enabled: window.pickerBar?.naming ?? false
        function onRevisionChanged() {
            const picked = list.model.selectedRows();
            if (picked.length === 1 && !picked[0].dir)
                window.pickerBar.setName(picked[0].name);
        }
    }
    Shortcut {
        sequence: "Escape"
        enabled: window.picking && !window.confirming && !navigation.editing
        onActivated: window.closed()
    }
    onConfirmingChanged: {
        if (confirming)
            confirmDialog.shown = confirming.asking;
    }

    Component {
        id: tabComponent
        FilesTab {}
    }
    Connections {
        target: window.navigator
        function onFolderChanged() {
            window.folder = window.navigator.folder;
        }
    }
    Connections {
        target: list
        function onShowHiddenChanged() {
            window.navigator.hidden = list.showHidden;
        }
    }

    // The quiet keyboard focus after a text field or a dialog lets go.
    function focusList() {
        if (pickerBar?.naming)
            pickerBar.focusName();
        else if (inTrash)
            trashList.focusList();
        else
            list.forceActiveFocus();
    }

    Repeater {
        model: Shortcuts.entries(Shortcuts.WINDOW)

        Item {
            id: entry

            required property var modelData

            Shortcut {
                sequences: entry.modelData.keys
                enabled: !window.asking || entry.modelData.asking === true
                onActivated: window.act(entry.modelData.action, [], null)
            }
        }
    }
    TapHandler {
        acceptedButtons: Qt.BackButton | Qt.ForwardButton
        onTapped: (point, button) => window.navigator.step(button === Qt.BackButton ? -1 : 1)
    }

    // The frame's background: behind the three rows, and the sidebar.
    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: window.inTrash ? trashCommands.bottom : commands.bottom
        color: window.frameColor
    }
    Rectangle {
        anchors.fill: sidebar
        color: window.frameColor
    }
    // Copies or moves (a Protocol.TRANSFER_* mode) `sources` (absolute
    // paths) into the folder `folder`, showing how it goes in this window.
    function transfer(mode: string, sources: var, folder: string) {
        transfers.start(mode, sources, folder);
    }

    TabRow {
        id: tabRow
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        tabs: window.tabs
        current: window.tab
        raised: navigation.color
        onPicked: picked => window.showTab(picked)
        onCloseRequested: closing => window.closeTab(closing)
        onNewRequested: window.newTab()
        onMoved: (moving, to) => window.moveTab(moving, to)
        onTornOut: torn => window.tearOut(torn)
    }
    // Extracts the archive at `path` beside it: into a folder named after
    // it ("Extract all"), or straight into its folder (`here`).
    function extract(path: string, here: bool) {
        transfer(here ? Protocol.TRANSFER_EXTRACT_HERE : Protocol.TRANSFER_EXTRACT, [path], Paths.parent(path));
    }

    // Packs `paths` into a new zip in the folder shown.
    function compress(paths: var) {
        if (archive === "" && paths.length > 0)
            transfer(Protocol.TRANSFER_COMPRESS, paths, folder);
    }

    // Does what a drop asks: `action` (a Protocol.TRANSFER_* mode or
    // Drops.TRASH) on `sources` (absolute paths) into `folder`.
    function drop(action: string, sources: var, folder: string) {
        // Nothing goes into or out of an archive by dragging (IMP-337).
        if (archive !== "" && (Paths.inside(folder, archive) || sources.some(source => Paths.inside(source, archive))))
            return;
        if (action === Drops.TRASH)
            FilesService.trash(sources, error => {
                if (error)
                    window.notice(null, error);
            });
        else
            transfer(action, sources, folder);
    }

    NavigationRow {
        id: navigation
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: tabRow.bottom
        navigator: window.navigator
        solid: FilesService.frameSolid
        canSearch: !window.inTrash
        onAddressClosed: window.focusList()
        onSearchLeft: {
            if (window.searching)
                results.focusResults();
            else
                window.focusList();
        }
    }
    CommandRow {
        id: commands
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: navigation.bottom
        solid: FilesService.frameSolid
        visible: !window.inTrash
        picked: list.model.selectedCount
        canPaste: FilesClipboard.hasFiles
        onCommandRequested: command => window.act(command, list.selection(), null)
        onNewRequested: at => window.openMenu(Menus.forNew(window.act), commands, at)
        onSortRequested: at => window.openMenu(Menus.forSort(list.model.sort, window.act), commands, at)
        extractable: window.extractable !== ""
        compressible: window.archive === "" && list.model.selectedCount > 0
        previewShown: window.previewShown
    }
    TrashCommandRow {
        id: trashCommands
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: navigation.bottom
        visible: window.inTrash
        model: trash
        solid: FilesService.frameSolid
        onEmptyRequested: window.empty()
        onRestoreRequested: window.restore()
        onEraseRequested: window.erase()
    }
    Sidebar {
        id: sidebar
        anchors.left: parent.left
        anchors.top: window.inTrash ? trashCommands.bottom : commands.bottom
        anchors.bottom: pickerLoader.top
        width: Tokens.filesSidebarWidth
        navigator: window.navigator
        onDriveMenuRequested: (drive, at) => window.openMenu(Menus.forDrive(drive, window.act), window.contentItem, at)
        onTabRequested: path => window.openTab(path, false)
        onDropRequested: (action, sources, folder) => window.drop(action, sources, folder)
    }
    FileList {
        id: list
        anchors.left: sidebar.right
        anchors.right: details.visible ? details.left : preview.active ? preview.left : parent.right
        compact: details.visible || preview.active
        anchors.top: commands.bottom
        visible: !window.inTrash && !window.searching
        anchors.bottom: pickerLoader.top
        folder: window.folder
        select: window.select
        focus: true
        picking: window.picking
        multiple: !window.picking || window.picker.multiple
        filter: window.picking ? (Picker.picksFolder(window.picker) ? Picker.ONLY_FOLDERS : Picker.filter(window.picker, window.pickerBar?.filterIndex ?? -1)) : null
        onFilesChosen: window.accept()
        onFolderOpened: path => window.navigator.visit(path, null)
        onTabRequested: path => window.openTab(path, false)
        onTrashRequested: rows => window.moveToTrash(rows)
        onDeleteRequested: rows => window.deleteForGood(rows)
        onDropRequested: (action, sources, folder) => window.drop(action, sources, folder)
        onCommandRequested: (command, rows) => window.act(command, rows, null)
        onRenameFailed: (row, error) => window.notice(row, error)
        onMenuRequested: (rows, at) => {
            if (rows.length === 1 && !rows[0].dir)
                menuApps.path = Paths.child(window.folder, rows[0].name);
            const items = rows.length > 0 ? Menus.forItems(rows, window.archive !== "", () => menuApps.apps, () => menuApps.defaultApp, window.act) : Menus.forFolder(FilesClipboard.hasFiles, list.showHidden, FilesService.history, window.act);
            window.openMenu(items, list, at);
        }
        solid: FilesService.listSolid
    }
    SearchResults {
        id: results
        anchors.left: sidebar.right
        anchors.right: parent.right
        anchors.top: commands.bottom
        anchors.bottom: parent.bottom
        visible: window.searching
        model: window.search
        folder: window.folder
        home: window.home
        solid: FilesService.listSolid
        onOpened: row => window.openResult(row)
        onLocationOpened: row => window.showResult(row)
        onClosed: {
            navigation.clearSearch();
            list.forceActiveFocus();
        }
    }
    Loader {
        id: preview
        anchors.right: parent.right
        anchors.top: commands.bottom
        anchors.bottom: parent.bottom
        width: Tokens.filesPreviewWidth
        active: window.previewShown && !window.inTrash
        sourceComponent: PreviewPane {
            folder: window.folder
            model: list.model
            solid: FilesService.listSolid
        }
    }
    // The apps for the item menu's Open with.
    OpenWithModel {
        id: menuApps
        service: FilesService
    }
    DetailsPane {
        id: details
        anchors.right: parent.right
        anchors.top: commands.bottom
        anchors.bottom: parent.bottom
        width: Tokens.filesDetailsWidth
        visible: window.detailsShown && !window.inTrash
        service: visible ? FilesService : null
        path: visible ? window.detailsOn : ""
        onCloseRequested: window.detailsShown = false
    }
    TrashModel {
        id: trash
        service: FilesService
        active: window.inTrash
    }
    TrashList {
        id: trashList
        anchors.left: sidebar.right
        anchors.right: parent.right
        anchors.top: trashCommands.bottom
        anchors.bottom: pickerLoader.top
        visible: window.inTrash
        model: trash
        home: window.home
        solid: FilesService.listSolid
        onEraseRequested: window.erase()
        onCommandRequested: command => window.act(command, [], null)
        onVisibleChanged: {
            if (visible)
                focusList();
        }
    }
    Loader {
        id: pickerLoader
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        active: window.picking
        height: active ? implicitHeight : 0

        sourceComponent: PickerBar {
            picker: window.picker
            filterIndex: window.picker.filter ?? 0
            canAccept: window.choice !== null && window.choice.action !== "none"
            solid: FilesService.frameSolid
            onAccepted: window.accept()
            onCancelled: window.closed()
        }
    }
    // Files dropped on the trash's list go to the trash.
    DropTarget {
        anchors.fill: trashList
        enabled: window.inTrash
        service: FilesService
        folderAt: () => Drops.place(Protocol.FILES_TRASH_PLACE, false)
        onDropRequested: (action, sources, folder) => window.drop(action, sources, folder)
    }
    Transfers {
        id: transfers
        anchors.fill: parent
    }

    Column {
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: Tokens.filesOverlayInset
        spacing: Tokens.gap

        Repeater {
            model: window.problems

            TrashNotice {
                required property var modelData

                problem: modelData
                onDismissed: window.problems = window.problems.filter(shown => shown !== modelData)
            }
        }
    }

    // Holds the window's clicks while the confirm dialog asks.
    MouseArea {
        anchors.fill: parent
        visible: window.confirming !== null
        acceptedButtons: Qt.AllButtons
        onWheel: wheel => wheel.accepted = true
    }
    ConfirmDialog {
        id: confirmDialog
        anchors.centerIn: parent
        enabled: window.confirming !== null
        visible: opacity > 0
        asking: shown
        opacity: window.confirming ? 1 : 0
        scale: window.confirming ? 1 : Tokens.filesClashEnterScale
        onAccepted: window.answer(true)
        onDeclined: window.answer(false)

        // What it asked, kept while it fades out after an answer.
        property var shown: Trash.confirm([])

        Behavior on opacity {
            NumberAnimation {
                duration: Tokens.durationPanel
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }
        Behavior on scale {
            NumberAnimation {
                duration: Tokens.durationPanel
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }
    }
    // The pane on the folder or an item in it reads it again as it changes.
    Connections {
        target: FilesService
        enabled: details.visible
        function onChanged(changed: var) {
            if (changed.view === list.model.view)
                details.reload();
        }
    }
    FilesMenu {
        id: menu
        anchors.fill: parent
        onClosed: list.forceActiveFocus()
    }
    PropertiesModel {
        id: propertiesModel
    }
    // A click beside the Properties dialog closes it.
    MouseArea {
        anchors.fill: parent
        visible: propertiesModel.subject !== null
        acceptedButtons: Qt.AllButtons
        onClicked: window.closeProperties()
        onWheel: wheel => wheel.accepted = true
    }
    PropertiesDialog {
        id: propertiesDialog
        anchors.centerIn: parent
        visible: propertiesModel.subject !== null
        width: Math.min(implicitWidth, window.width - 2 * Tokens.filesOverlayInset)
        height: implicitHeight
        maxHeight: window.height - 2 * Tokens.filesOverlayInset
        subject: propertiesModel.subject ?? Props.subject(window.folder, [], null)
        properties: propertiesModel.properties
        counted: propertiesModel.counted
        changing: propertiesModel.changing
        apps: propertiesModel.openWith.apps
        defaultApp: propertiesModel.openWith.defaultApp
        onCloseRequested: window.closeProperties()
        onAccessFlipped: (who, right) => propertiesModel.flipAccess(who, right)
        onDefaultChosen: app => propertiesModel.openWith.makeDefault(app)
    }
    EjectProblem {
        id: ejectProblem
        anchors.fill: parent
    }
}
