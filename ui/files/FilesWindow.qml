pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import "../theme"
import "service"
import "paths.js" as Paths
import "tabs.js" as Tabs
import "trash.js" as Trash
import "properties.js" as Props
import "listing.js" as Listing
import "../theme/icons.js" as Icons
import "games.js" as Games
import "home.js" as Home
import "search.js" as Search
import "../daemon/protocol.js" as Protocol

// A Nova Files window: an app window the compositor tiles and rounds,
// showing `folder` with `select` selected. The tab row, the navigation
// row, the command row, then the sidebar beside the file list, with its
// copies and moves over them all; opening a folder in the list shows it in
// the same window.
// On the Home page (Home.isHome, where `nova files` without a path opens)
// the Home page takes the file list's place.
// Inside an archive (a zip or tar, opened like a folder) nothing can be
// changed: its command row offers "Extract all" instead.
// At the trash, the trash's command row and list take the place of the
// folder's, and anything deleted for good is asked about first
// (ConfirmDialog). At the games, theirs do, and a game's menu goes to its
// install or save folder. The Properties dialog (Alt+Enter, a drive's
// menu), a drive's menu, the dialog when one can't be ejected, a folder's
// pin menu and a game's menu lie over it all.
// The preview pane (Alt+P, or the command row's button) sits at the file
// list's right, loaded only while it shows. What uses space (the command
// row's button, or opening a drive) stays on as the window moves, until
// the button turns it off.
// Each tab (FilesTab) has its own Navigator; the rows, the sidebar and the
// one file list follow the shown tab's, and the list's order and
// selection are kept with a tab as it goes to the back (Ctrl+T, Ctrl+W,
// Ctrl+Tab, Ctrl+Shift+Tab; a middle click on a folder opens it in a tab
// behind). Closing the last tab closes the window; a tab torn out of the
// row asks for a window of its own (`windowRequested`), carrying its
// history, order and selection.
// The View menu (and Explorer's Ctrl+Shift+1 to 4 and 6) switch the file
// list between Icons and Details for the window, in every folder it shows.
// While something is typed in the search field, the search results take
// the file list's place (SearchResults); moving to another folder empties
// the field.
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
    readonly property bool inHome: Home.isHome(folder)
    readonly property bool inGames: Games.isGames(folder)
    readonly property bool inFolder: !Home.isPage(folder)
    // The command row shown: the trash's, the games' or the folder's.
    readonly property Item commandRow: inTrash ? trashCommands : inGames ? gamesCommands : commands
    property bool spaceView: false
    readonly property bool searching: search.active && !inTrash
    readonly property SearchModel search: SearchModel {
        service: FilesService
        query: navigation.query
        within: results.everywhere ? "" : window.folder
    }
    readonly property string home: Quickshell.env("HOME") ?? ""
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
        focusContent();
    }

    // Gives the keyboard back to the list shown: the folder's or the trash's.
    function focusContent() {
        if (inTrash)
            trashList.focusList();
        else if (inGames)
            gamesList.focusList();
        else if (inHome)
            homePage.forceActiveFocus();
        else
            list.forceActiveFocus();
    }

    // Shows the Properties of `rows` (the file list's) in the folder shown,
    // of the folder itself when there are none, or of `drive`.
    function showProperties(rows: var, drive: var) {
        propertiesModel.show(Props.subject(folder, rows, drive));
        propertiesDialog.forceActiveFocus();
    }

    function closeProperties() {
        propertiesModel.close();
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
    }
    // A game's menu doesn't outlive the games.
    onInGamesChanged: gameMenu.close()
    onFolderChanged: navigation.clearSearch()
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
    Shortcut {
        sequence: "Alt+P"
        enabled: !window.inTrash
        onActivated: window.previewShown = !window.previewShown
    }
    // Explorer's icon sizes (Nova has one) and Details, while the file
    // list has the keyboard.
    Shortcut {
        sequences: ["Ctrl+Shift+1", "Ctrl+Shift+2", "Ctrl+Shift+3", "Ctrl+Shift+4"]
        enabled: list.activeFocus
        onActivated: list.view = Listing.ICONS
    }
    Shortcut {
        sequence: "Ctrl+Shift+6"
        enabled: list.activeFocus
        onActivated: list.view = Listing.DETAILS
    }
    Shortcut {
        sequences: ["Ctrl+F", "F3"]
        enabled: !window.inTrash
        onActivated: navigation.focusSearch()
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

    NavigationRow {
        id: navigation
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: tabRow.bottom
        navigator: window.navigator
        solid: FilesService.frameSolid
        canSearch: !window.inTrash
        onAddressClosed: window.focusContent()
        onSearchLeft: {
            if (window.searching)
                results.focusResults();
            else
                list.forceActiveFocus();
        }
    }
    CommandRow {
        id: commands
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: navigation.bottom
        solid: FilesService.frameSolid
        visible: !window.inTrash && !window.inGames
        extractable: window.extractable !== ""
        compressible: window.archive === "" && list.model.revision >= 0 && list.model.selectedCount > 0
        onExtractAllRequested: window.extract(window.extractable, false)
        onCompressRequested: window.compress()
        previewShown: window.previewShown
        onPreviewToggled: window.previewShown = !window.previewShown
        view: list.view
        onViewMenuRequested: at => viewMenu.open(commands.mapToItem(viewMenu, at))
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
        onTabRequested: path => window.openTab(path, false)
        onPinMenuRequested: (path, at) => pinMenu.open(path, at)
        onDriveOpened: id => window.openDrive(id)
    }
    FileList {
        id: list
        anchors.left: sidebar.right
        anchors.right: preview.active ? preview.left : parent.right
        anchors.top: commands.bottom
        visible: window.inFolder && !window.searching
        anchors.bottom: parent.bottom
        // Nothing to read while a page shows.
        folder: Home.isPage(window.folder) ? "" : window.folder
        select: window.select
        focus: true
        onFolderOpened: path => window.navigator.visit(path, null)
        onTabRequested: path => window.openTab(path, false)
        onTrashRequested: rows => window.moveToTrash(rows)
        onDeleteRequested: rows => window.deleteForGood(rows)
        onPropertiesRequested: rows => window.showProperties(rows, null)
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
        subject: propertiesModel.subject ?? Props.subject(window.folder, [], null)
        properties: propertiesModel.properties
        counted: propertiesModel.counted
        apps: propertiesModel.openWith.apps
        defaultApp: propertiesModel.openWith.defaultApp
        onCloseRequested: window.closeProperties()
        onAccessFlipped: (who, right) => propertiesModel.flipAccess(who, right)
        onDefaultChosen: app => propertiesModel.openWith.makeDefault(app)
    }
    PopupMenu {
        id: viewMenu
        anchors.fill: parent

        onHidden: window.focusContent()

        function choose(view: string) {
            list.view = view;
            shown = false;
        }

        function open(at: point) {
            place(at);
            shown = true;
        }
        onDismissed: shown = false

        MenuRow {
            icon: Icons.GRID
            label: qsTr("Icons")
            checked: list.view === Listing.ICONS
            onChosen: viewMenu.choose(Listing.ICONS)
        }
        MenuRow {
            icon: Icons.VIEW
            label: qsTr("Details")
            checked: list.details
            onChosen: viewMenu.choose(Listing.DETAILS)
        }
    }
    DriveMenu {
        id: driveMenu
        anchors.fill: parent
        onPropertiesChosen: drive => window.showProperties([], drive)
        onHidden: window.focusContent()
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
