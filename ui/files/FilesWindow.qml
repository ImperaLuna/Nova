pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../theme"
import "service"
import "paths.js" as Paths
import "tabs.js" as Tabs
import "trash.js" as Trash
import "../daemon/protocol.js" as Protocol

// A Nova Files window: an app window the compositor tiles and rounds,
// showing `folder` with `select` selected. The tab row, the navigation
// row, the command row, then the sidebar beside the file list, with its
// copies and moves over them all; opening a folder in the list shows it in
// the same window.
// At the trash, the trash's command row and list take the place of the
// folder's, and anything deleted for good is asked about first
// (ConfirmDialog). A drive's menu and the dialog when one can't be ejected
// lie over it all.
// Each tab (FilesTab) has its own Navigator; the rows, the sidebar and the
// one file list follow the shown tab's, and the list's order and
// selection are kept with a tab as it goes to the back (Ctrl+T, Ctrl+W,
// Ctrl+Tab, Ctrl+Shift+Tab; a middle click on a folder opens it in a tab
// behind). Closing the last tab closes the window; a tab torn out of the
// row asks for a window of its own (`windowRequested`), carrying its
// history, order and selection.
// `folder` follows the window's Navigator as it moves (Alt+Left,
// Alt+Right, Alt+Up and the mouse's back and forward buttons too). Its
// frame and its file list each go from glass to solid as config.toml's
// [files] says, live. Each draws its own background, so one never shows
// through the other.
FloatingWindow {
    id: window

    required property string folder
    property var select: []
    // A tab torn out of another window that this one starts as:
    // {history, view, hidden}, or null.
    property var carried: null

    readonly property FilesTab firstTab: FilesTab {}
    property var tabs: [firstTab]
    property FilesTab tab: firstTab
    readonly property Navigator navigator: tab.navigator
    readonly property color frameColor: Tokens.mix(Tokens.glass, Tokens.filesSolidFrame, FilesService.frameSolid)

    readonly property bool inTrash: Trash.isTrash(folder)
    // What the confirm dialog asks, and what a yes does: {asking, act}.
    property var confirming: null
    // Notices of what the trash couldn't do, oldest first: {row, error}.
    property var problems: []

    // A torn out tab wants a window like this one, starting as `carried`.
    signal windowRequested(string folder, var select, var carried)

    title: tab.title
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

    // Opens `path` (absolute, or from `~`) in a new tab at the end, shown
    // at once when `front`.
    function openTab(path: string, front: bool) {
        const opened = tabComponent.createObject(window);
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

    // Brings `next` to the front, keeping the shown tab's view with it.
    function showTab(next: FilesTab) {
        if (next === tab)
            return;
        tab.view = list.view();
        front(next);
    }

    function front(next: FilesTab) {
        next.navigator.hidden = list.showHidden;
        tab = next;
        folder = next.navigator.folder;
        list.show(next.view);
    }

    function cycleTabs(steps: int) {
        showTab(tabs[Tabs.cycled(tabs.indexOf(tab), tabs.length, steps)]);
    }

    function closeTab(closing: FilesTab) {
        const at = tabs.indexOf(closing);
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
        tabs = Tabs.reordered(tabs, tabs.indexOf(moving), to);
    }

    // Gives `torn` a window of its own and closes it here.
    function tearOut(torn: FilesTab) {
        const view = torn === tab ? list.view() : torn.view;
        const place = torn.navigator.folder;
        windowRequested(place, (view?.selected ?? []).map(name => Paths.child(place, name)), {
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

    Component.onCompleted: {
        if (carried) {
            list.showHidden = carried.hidden;
            navigator.hidden = carried.hidden;
            navigator.resume(carried.history);
            if (carried.view)
                list.model.sort = carried.view.sort;
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

    Shortcut {
        sequence: "Ctrl+T"
        onActivated: window.newTab()
    }
    Shortcut {
        sequence: "Ctrl+W"
        onActivated: window.closeTab(window.tab)
    }
    Shortcut {
        sequence: "Ctrl+Tab"
        onActivated: window.cycleTabs(1)
    }
    Shortcut {
        // Shift+Tab comes as Backtab.
        sequences: ["Ctrl+Shift+Tab", "Ctrl+Backtab", "Ctrl+Shift+Backtab"]
        onActivated: window.cycleTabs(-1)
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
    NavigationRow {
        id: navigation
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: tabRow.bottom
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
    }
    FileList {
        id: list
        anchors.left: sidebar.right
        anchors.right: parent.right
        anchors.top: commands.bottom
        visible: !window.inTrash
        anchors.bottom: parent.bottom
        folder: window.folder
        select: window.select
        focus: true
        onFolderOpened: path => window.navigator.visit(path, null)
        onTabRequested: path => window.openTab(path, false)
        onTrashRequested: rows => window.moveToTrash(rows)
        onDeleteRequested: rows => window.deleteForGood(rows)
        solid: FilesService.listSolid
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
    DriveMenu {
        id: driveMenu
        anchors.fill: parent
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
