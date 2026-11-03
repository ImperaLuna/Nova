pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../theme"
import "service"
import "paths.js" as Paths
import "games.js" as Games
import "trash.js" as Trash
import "home.js" as Home
import "../daemon/protocol.js" as Protocol

// A Nova Files window: an app window the compositor tiles and rounds,
// showing `folder` with `select` selected. The navigation row, the command
// row, then the sidebar beside the file list, with its copies and moves
// over them all; opening a folder in the list shows it in the same window.
// On the Home page (Home.isHome, where `nova files` without a path opens)
// the Home page takes the file list's place. At the trash, the trash's
// command row and list take the place of the folder's, and anything
// deleted for good is asked about first (ConfirmDialog). At the games,
// theirs do, and a game's menu goes to its install or save folder. A
// drive's menu and the dialog when one can't be ejected, a folder's pin
// menu and a game's menu lie over it all. What uses space (the command
// row's button, or opening a drive) stays on as the window moves, until
// the button turns it off.
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
    readonly property bool inHome: Home.isHome(folder)
    readonly property bool inGames: Games.isGames(folder)
    readonly property bool inFolder: !Home.isPage(folder)
    // The command row shown: the trash's, the games' or the folder's.
    readonly property Item commandRow: inTrash ? trashCommands : inGames ? gamesCommands : commands
    property bool spaceView: false
    // What the confirm dialog asks, and what a yes does: {asking, act}.
    property var confirming: null
    // Notices of what the trash couldn't do, oldest first: {row, error}.
    property var problems: []

    title: inFolder ? Paths.folderName(folder) : Home.pageName(folder)
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

    // Opens the drive `id`, showing what uses space on it.
    function openDrive(id: string) {
        spaceView = true;
        navigator.openDrive(id);
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

    // Gives the keyboard to the list shown.
    function focusList() {
        if (inTrash)
            trashList.focusList();
        else if (inGames)
            gamesList.focusList();
        else if (inHome)
            homePage.forceActiveFocus();
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
    // A game's menu doesn't outlive the games.
    onInGamesChanged: gameMenu.close()
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
        anchors.bottom: window.commandRow.bottom
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
        onAddressClosed: window.focusList()
    }
    CommandRow {
        id: commands
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: navigation.bottom
        solid: FilesService.frameSolid
        visible: !window.inTrash && !window.inGames
        spaceView: window.spaceView
        onSpaceToggled: window.spaceView = !window.spaceView
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
    GamesCommandRow {
        id: gamesCommands
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: navigation.bottom
        visible: window.inGames
        model: games
        solid: FilesService.frameSolid
        onChosen: folder => window.navigator.visit(folder, null)
    }
    Sidebar {
        id: sidebar
        anchors.left: parent.left
        anchors.top: window.commandRow.bottom
        anchors.bottom: parent.bottom
        width: Tokens.filesSidebarWidth
        navigator: window.navigator
        onDriveMenuRequested: (drive, at) => driveMenu.open(drive, at)
        onPinMenuRequested: (path, at) => pinMenu.open(path, at)
        onDriveOpened: id => window.openDrive(id)
    }
    FileList {
        id: list
        anchors.left: sidebar.right
        anchors.right: parent.right
        anchors.top: commands.bottom
        visible: window.inFolder
        anchors.bottom: parent.bottom
        // Nothing to read while a page shows.
        folder: Home.isPage(window.folder) ? "" : window.folder
        select: window.select
        focus: true
        onFolderOpened: path => window.navigator.visit(path, null)
        onTrashRequested: rows => window.moveToTrash(rows)
        onDeleteRequested: rows => window.deleteForGood(rows)
        solid: FilesService.listSolid
        spaceView: window.spaceView
        // Back from a page (Home, the games, the trash): keys go to the
        // list at once.
        onVisibleChanged: {
            if (visible)
                forceActiveFocus();
        }
    }
    HomePage {
        id: homePage
        anchors.left: sidebar.right
        anchors.right: parent.right
        anchors.top: commands.bottom
        anchors.bottom: parent.bottom
        visible: window.inHome
        navigator: window.navigator
        solid: FilesService.listSolid
        onPinMenuRequested: (path, at) => pinMenu.open(path, at)
        onDriveMenuRequested: (drive, at) => driveMenu.open(drive, at)
        onVisibleChanged: {
            if (visible)
                forceActiveFocus();
        }
        onDriveOpened: id => window.openDrive(id)
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
    GamesModel {
        id: games
        service: FilesService
        active: window.inGames
    }
    GamesList {
        id: gamesList
        anchors.left: sidebar.right
        anchors.right: parent.right
        anchors.top: gamesCommands.bottom
        anchors.bottom: parent.bottom
        visible: window.inGames
        model: games
        home: Quickshell.env("HOME") ?? ""
        solid: FilesService.listSolid
        onOpened: index => window.navigator.visit(games.rows.get(index).folder, null)
        onMenuRequested: (index, at) => {
            games.pick(index);
            gameMenu.open(games.pickedRow, at);
        }
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
    GameMenu {
        id: gameMenu
        anchors.fill: parent
        onChosen: folder => window.navigator.visit(folder, null)
    }
    PinMenu {
        id: pinMenu
        anchors.fill: parent
    }
}
