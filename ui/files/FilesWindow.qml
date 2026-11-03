pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../theme"
import "service"
import "paths.js" as Paths
import "trash.js" as Trash
import "picker.js" as Picker
import "../daemon/protocol.js" as Protocol

// A Nova Files window: an app window the compositor tiles and rounds,
// showing `folder` with `select` selected. The navigation row, the command
// row, then the sidebar beside the file list, with its copies and moves
// over them all; opening a folder in the list shows it in the same window.
// At the trash, the trash's command row and list take the place of the
// folder's, and anything deleted for good is asked about first
// (ConfirmDialog). A drive's menu and the dialog when one can't be ejected
// lie over it all.
// `folder` follows the window's Navigator as it moves (Alt+Left,
// Alt+Right, Alt+Up and the mouse's back and forward buttons too). Its
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

    title: picking ? Picker.title(picker) : inTrash ? Trash.name() : Paths.folderName(folder)
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
        if (pickerBar?.naming)
            pickerBar.focusName();
        else if (inTrash)
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
        navigator.begin(folder);
        if (pickerBar?.naming)
            pickerBar.focusName();
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
        onDriveMenuRequested: (drive, at) => driveMenu.open(drive, at)
    }
    FileList {
        id: list
        anchors.left: sidebar.right
        anchors.right: parent.right
        anchors.top: commands.bottom
        visible: !window.inTrash
        anchors.bottom: pickerLoader.top
        folder: window.folder
        select: window.select
        focus: true
        picking: window.picking
        multiple: !window.picking || window.picker.multiple
        filter: window.picking ? (Picker.picksFolder(window.picker) ? Picker.ONLY_FOLDERS : Picker.filter(window.picker, window.pickerBar?.filterIndex ?? -1)) : null
        onFilesChosen: window.accept()
        onFolderOpened: path => window.navigator.visit(path, null)
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
        anchors.bottom: pickerLoader.top
        visible: window.inTrash
        model: trash
        solid: FilesService.listSolid
        onEraseRequested: window.erase()
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
