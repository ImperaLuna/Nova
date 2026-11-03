pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../theme"
import "service"
import "paths.js" as Paths
import "trash.js" as Trash
import "commands.js" as Commands
import "menus.js" as Menus
import "../daemon/protocol.js" as Protocol

// A Nova Files window: an app window the compositor tiles and rounds,
// showing `folder` with `select` selected. The navigation row, the command
// row, then the sidebar beside the file list, with its copies and moves
// over them all; opening a folder in the list shows it in the same window.
// At the trash, the trash's command row and list take the place of the
// folder's, and anything deleted for good is asked about first
// (ConfirmDialog). Right-click menus (menus.js: on items, on the empty
// list, on a drive, and the command row's New and Sort) and the dialog
// when a drive can't be ejected lie over it all. Show details opens the
// details pane on the right of the list, on the picked item or the folder.
// Cut, copy and paste go through the system clipboard (FilesClipboard);
// pasting copies or moves into the folder shown, like dropping there.
// `folder` follows the window's Navigator as it moves (Alt+Left,
// Alt+Right, Alt+Up and the mouse's back and forward buttons too). Its
// frame and its file list each go from glass to solid as config.toml's
// [files] says, live. Each draws its own background, so one never shows
// through the other.
FloatingWindow {
    id: window

    required property string folder
    property var select: []
    readonly property Navigator navigator: Navigator {
        hidden: list.showHidden
        onFolderChanged: window.folder = folder
    }
    readonly property color frameColor: Tokens.mix(Tokens.glass, Tokens.filesSolidFrame, FilesService.frameSolid)

    readonly property bool inTrash: Trash.isTrash(folder)
    // What the confirm dialog asks, and what a yes does: {asking, act}.
    property var confirming: null
    // Notices of what the trash couldn't do, oldest first: {row, error}.
    property var problems: []
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
            else
                for (const row of rows.filter(row => row.dir))
                    FilesService.openWindow(Paths.child(folder, row.name));
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
        case Commands.NEW_FOLDER:
            FilesService.newFolder(folder, (error, name) => {
                if (error)
                    window.notice(null, error);
                else
                    list.rename(name);
            });
            break;
        case Commands.DETAILS:
            if (extra)
                window.onDrive(extra, mount => window.showDetails(mount));
            else
                window.showDetails("");
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

    title: inTrash ? Trash.name() : Paths.folderName(folder)
    implicitWidth: Tokens.filesWindowWidth
    implicitHeight: Tokens.filesWindowHeight
    color: "transparent"

    // Moves the rows (the file list's) in the folder shown to the trash, at
    // once: Delete.
    function moveToTrash(rows: var) {
        const paths = Trash.paths(folder, rows);
        FilesService.trash(paths, error => {
            if (error)
                window.notice(rows.length === 1 ? rows[0] : null, error);
        });
    }

    // Asks, then deletes the rows (the file list's) in the folder shown for
    // good: Shift+Delete.
    function deleteForGood(rows: var) {
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

    function confirm(rows: var, act: var) {
        confirming = {
            asking: Trash.confirm(rows),
            act: act
        };
        confirmDialog.forceActiveFocus();
    }

    function answer(yes: bool) {
        const act = confirming?.act;
        confirming = null;
        if (yes)
            act?.();
        if (inTrash)
            trashList.focusList();
        else
            list.forceActiveFocus();
    }

    function notice(row: var, error: string) {
        problems = problems.concat([
            {
                row: row,
                error: error
            }
        ]);
    }

    Component.onCompleted: navigator.begin(folder)
    onFolderChanged: detailsPath = ""
    onConfirmingChanged: {
        if (confirming)
            confirmDialog.shown = confirming.asking;
    }

    Shortcut {
        sequence: "Alt+Left"
        onActivated: window.navigator.step(-1)
    }
    Shortcut {
        sequence: "Alt+Right"
        onActivated: window.navigator.step(1)
    }
    Shortcut {
        sequence: "Alt+Up"
        onActivated: window.navigator.up()
    }
    Shortcut {
        sequences: ["Ctrl+L", "Alt+D"]
        onActivated: navigation.editAddress()
    }
    TapHandler {
        acceptedButtons: Qt.BackButton | Qt.ForwardButton
        onTapped: (point, button) => window.navigator.step(button === Qt.BackButton ? -1 : 1)
    }

    // The frame's background: behind the two rows, and the sidebar.
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

    NavigationRow {
        id: navigation
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        navigator: window.navigator
        solid: FilesService.frameSolid
        onAddressClosed: list.forceActiveFocus()
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
        anchors.bottom: parent.bottom
        width: Tokens.filesSidebarWidth
        navigator: window.navigator
        onDriveMenuRequested: (drive, at) => window.openMenu(Menus.forDrive(drive, window.act), window.contentItem, at)
    }
    FileList {
        id: list
        anchors.left: sidebar.right
        anchors.right: details.visible ? details.left : parent.right
        anchors.top: commands.bottom
        visible: !window.inTrash
        anchors.bottom: parent.bottom
        folder: window.folder
        select: window.select
        focus: true
        onFolderOpened: path => window.navigator.visit(path, null)
        onTrashRequested: rows => window.moveToTrash(rows)
        onDeleteRequested: rows => window.deleteForGood(rows)
        onCommandRequested: (command, rows) => window.act(command, rows, null)
        onRenameFailed: (row, error) => window.notice(row, error)
        onMenuRequested: (rows, at) => {
            if (rows.length === 1 && !rows[0].dir)
                menuApps.path = Paths.child(window.folder, rows[0].name);
            const items = rows.length > 0 ? Menus.forItems(rows, () => menuApps.apps, () => menuApps.defaultApp, window.act) : Menus.forFolder(FilesClipboard.hasFiles, list.showHidden, window.act);
            window.openMenu(items, list, at);
        }
        solid: FilesService.listSolid
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
        anchors.bottom: parent.bottom
        visible: window.inTrash
        model: trash
        solid: FilesService.listSolid
        onEraseRequested: window.erase()
        onVisibleChanged: {
            if (visible)
                focusList();
        }
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
    FilesMenu {
        id: menu
        anchors.fill: parent
        onClosed: list.forceActiveFocus()
    }
    EjectProblem {
        id: ejectProblem
        anchors.fill: parent
    }
}
