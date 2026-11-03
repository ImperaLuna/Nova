pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../theme"
import "service"
import "paths.js" as Paths
import "tabs.js" as Tabs
import "drops.js" as Drops
import "trash.js" as Trash
import "properties.js" as Props
import "shortcuts.js" as Shortcuts
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
// (ConfirmDialog). The Properties dialog (Alt+Enter, a drive's menu), a
// drive's menu and the dialog when one can't be ejected lie over it all.
// Files dropped on the list or the sidebar are copied, moved or trashed
// here, as the drop decided (drops.js).
// The preview pane (Alt+P, or the command row's button) sits at the file
// list's right, loaded only while it shows.
// Each tab (FilesTab) has its own Navigator; the rows, the sidebar and the
// one file list follow the shown tab's, and the list's order and
// selection are kept with a tab as it goes to the back (the table's tab
// keys; a middle click on a folder opens it in a tab
// behind). Closing the last tab closes the window; a tab torn out of the
// row asks for a window of its own (`windowRequested`), carrying its
// history, order and selection.
// `folder` follows the window's Navigator as it moves (Alt+Left,
// Alt+Right, Alt+Up and the mouse's back and forward buttons too). The
// window's keys are the table's WINDOW entries (shortcuts.js), done by
// act(); the lists answer their own, and hand it theirs that are the
// window's, like undo and redo (Ctrl+Z, Ctrl+Y), which novad does for
// every window alike; what can't be undone gets a notice. Its
// frame and its file list each go from glass to solid as config.toml's
// [files] says, live. Each draws its own background, so one never shows
// through the other.
FloatingWindow {
    id: window

    required property string folder
    property var select: []
    // A tab torn out of another window that this one starts as:
    // {history, view (from snapshot()), hidden}, or null.
    property var carried: null

    readonly property FilesTab firstTab: FilesTab {}
    property var tabs: [firstTab]
    property FilesTab tab: firstTab
    readonly property Navigator navigator: tab.navigator
    readonly property color frameColor: Tokens.mix(Tokens.glass, Tokens.filesSolidFrame, FilesService.frameSolid)

    readonly property bool inTrash: Trash.isTrash(folder)
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
    // name clash, the Properties dialog, a drive's menu or its eject problem.
    readonly property bool asking: confirming !== null || transfers.asking !== null || propertiesModel.subject !== null || driveMenu.visible || ejectProblem.visible
    // What the confirm dialog asks, and what a yes does: {asking, act}.
    property var confirming: null
    // Notices of what the trash couldn't do, oldest first: {row, error}.
    property var problems: []
    property bool previewShown: false

    // A torn out tab wants a window like this one, starting as `carried`.
    signal windowRequested(string folder, var select, var carried)

    title: tab.title
    implicitWidth: Tokens.filesWindowWidth
    implicitHeight: Tokens.filesWindowHeight
    color: "transparent"

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
            list: list.view(),
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
        list.show(next.view?.list ?? null, kept?.list.selected ?? []);
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

    Component.onCompleted: {
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

    // Does a shortcut table action (Shortcuts.TABLE) that is the window's.
    function act(action: string) {
        switch (action) {
        case "back":
            navigator.step(-1);
            break;
        case "forward":
            navigator.step(1);
            break;
        case "up":
            navigator.up();
            break;
        case "address":
            navigation.editAddress();
            break;
        case "search":
            navigation.editSearch();
            break;
        case "refresh":
            if (inTrash)
                trash.refresh();
            else
                list.refresh();
            break;
        case "newWindow":
            // From the trash, on the folder its tree is on, or home.
            FilesService.showFolder(inTrash ? (navigator.treeFolder || home) : folder);
            break;
        case "undo":
        case "redo":
            transfers.undo(action === "redo", error => notice(null, error));
            break;
        case "newTab":
            newTab();
            break;
        case "closeTab":
            closeTab(tab);
            break;
        case "nextTab":
            cycleTabs(1);
            break;
        case "previousTab":
            cycleTabs(-1);
            break;
        case "preview":
            // The trash has no preview pane.
            if (!inTrash)
                previewShown = !previewShown;
            break;
        default:
            console.warn(`nova: files: no window action ${action}`);
        }
    }

    // The quiet keyboard focus after a text field or a dialog lets go.
    function focusList() {
        if (inTrash)
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
                onActivated: window.act(entry.modelData.action)
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

    // Packs the picked rows into a new zip in the folder shown.
    function compress() {
        const paths = list.model.selectedRows().map(row => Paths.child(folder, row.name));
        if (archive === "" && paths.length > 0)
            transfer(Protocol.TRANSFER_COMPRESS, paths, folder);
    }

    // Does what a drop decided: `action` (Drops.decide) on `sources`
    // (absolute paths) into `folder`.
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
        onAddressClosed: window.focusList()
        onSearchClosed: window.focusList()
    }
    CommandRow {
        id: commands
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: navigation.bottom
        solid: FilesService.frameSolid
        visible: !window.inTrash
        extractable: window.extractable !== ""
        compressible: window.archive === "" && list.model.revision >= 0 && list.model.selectedCount > 0
        onExtractAllRequested: window.extract(window.extractable, false)
        onCompressRequested: window.compress()
        previewShown: window.previewShown
        onPreviewToggled: window.previewShown = !window.previewShown
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
        onDriveMenuRequested: (drive, at) => driveMenu.open(drive, at)
        onTabRequested: path => window.openTab(path, false)
        onDropRequested: (action, sources, folder) => window.drop(action, sources, folder)
    }
    FileList {
        id: list
        anchors.left: sidebar.right
        anchors.right: preview.active ? preview.left : parent.right
        anchors.top: commands.bottom
        visible: !window.inTrash
        anchors.bottom: parent.bottom
        folder: window.folder
        select: window.select
        focus: true
        onFolderOpened: path => window.navigator.visit(path, null)
        onTabRequested: path => window.openTab(path, false)
        onTrashRequested: rows => window.moveToTrash(rows)
        onActionRequested: action => window.act(action)
        onDeleteRequested: rows => window.deleteForGood(rows)
        onPropertiesRequested: rows => window.showProperties(rows, null)
        onDropRequested: (action, sources, folder) => window.drop(action, sources, folder)
        solid: FilesService.listSolid
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
        home: window.home
        solid: FilesService.listSolid
        onEraseRequested: window.erase()
        onActionRequested: action => window.act(action)
        onVisibleChanged: {
            if (visible)
                focusList();
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
    DriveMenu {
        id: driveMenu
        anchors.fill: parent
        onPropertiesChosen: drive => window.showProperties([], drive)
        onEjectChosen: drive => FilesService.eject(drive.id, outcome => {
                if (outcome === Protocol.EJECT_BUSY)
                    ejectProblem.show(drive.name);
            })
    }
    EjectProblem {
        id: ejectProblem
        anchors.fill: parent
    }
}
