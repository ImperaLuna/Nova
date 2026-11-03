pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../theme"
import "service"
import "paths.js" as Paths
import "drops.js" as Drops
import "trash.js" as Trash
import "shortcuts.js" as Shortcuts
import "../daemon/protocol.js" as Protocol

// A Nova Files window: an app window the compositor tiles and rounds,
// showing `folder` with `select` selected. The navigation row, the command
// row, then the sidebar beside the file list, with its copies and moves
// over them all; opening a folder in the list shows it in the same window.
// At the trash, the trash's command row and list take the place of the
// folder's, and anything deleted for good is asked about first
// (ConfirmDialog). A drive's menu and the dialog when one can't be ejected
// lie over it all. Files dropped on the list or the sidebar are copied,
// moved or trashed here, as the drop decided (drops.js).
// `folder` follows the window's Navigator as it moves (Alt+Left,
// Alt+Right, Alt+Up and the mouse's back and forward buttons too). The
// window's keys are the table's WINDOW entries (shortcuts.js), done by
// act(); the lists answer their own. Its
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

    Component.onCompleted: navigator.begin(folder)
    onConfirmingChanged: {
        if (confirming)
            confirmDialog.shown = confirming.asking;
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
            // From the trash, on the folder its tree is on.
            FilesService.showFolder(inTrash ? navigator.treeFolder : folder);
            break;
        case "closeWindow":
            // Hiding a window from QML doesn't say `closed` (only the
            // compositor closing it does), which files.qml waits for.
            closed();
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
                enabled: window.confirming === null || entry.modelData.asking === true
                onActivated: window.act(entry.modelData.action)
            }
        }
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

    // Does what a drop decided: `action` (Drops.decide) on `sources`
    // (absolute paths) into `folder`.
    function drop(action: string, sources: var, folder: string) {
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
        anchors.top: parent.top
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
        onDropRequested: (action, sources, folder) => window.drop(action, sources, folder)
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
        onTrashRequested: rows => window.moveToTrash(rows)
        onBackRequested: window.navigator.step(-1)
        onDeleteRequested: rows => window.deleteForGood(rows)
        onDropRequested: (action, sources, folder) => window.drop(action, sources, folder)
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
        home: Quickshell.env("HOME") ?? ""
        solid: FilesService.listSolid
        onEraseRequested: window.erase()
        onBackRequested: window.navigator.step(-1)
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
