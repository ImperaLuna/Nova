pragma ComponentBehavior: Bound

import QtQuick
import "service"
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol
import "arrival.js" as Arrival
import "commands.js" as Commands
import "drops.js" as Drops
import "listing.js" as Listing
import "paths.js" as Paths
import "shortcuts.js" as Shortcuts
import "space.js" as Space

// Nova Files' file list: the folder's entries from novad (FolderModel) in
// one of two views (`view`, a Listing view), and the item count under
// them. Details shows rows under the column header (Name, then Modified,
// Type and Size at their widths; a click sorts by that column, again turns
// the order round); Icons shows tiles, line after line, in the same order.
// A click selects an entry, Ctrl adds or drops one, Shift selects a run.
// Its keys are the table's LIST entries (shortcuts.js): the arrows (Left
// and Right too in Icons), Page Up/Down, Home and End move the selection
// (Shift extends it), Ctrl+A selects all, Ctrl+H shows or hides dot files,
// and typing a name goes to it (NameTyping). A double-click or Enter
// opens: a folder is gone into (`folderOpened`), files open in their
// default app. Delete asks to move the selection to the trash
// (`trashRequested`), Shift+Delete to delete it for good
// (`deleteRequested`). F5 (the window's) reads the folder again
// (`refresh`). A right-click asks for a menu (`menuRequested`): on an
// item, for the selection, picking the item first unless it's already
// picked; on the empty list, for the folder. The other keys' commands go
// to the window as `commandRequested`. `rename(name)` turns that row's
// name into a field (F2, or a new folder), renamed through novad when it
// closes (in Details; Icons' tiles don't rename in place yet). Cut items
// read faded. A middle
// click on a folder asks for it in a new tab (`tabRequested`). tabView()
// and restoreTab() hand its order and selection to the window's tabs and
// back.
// Rows drag out (the selection, FileDrag) and files dragged in drop on the
// folder row under the pointer, or else the folder shown
// (`dropRequested`); resting on a folder row opens it. Pressing a
// selected row keeps the selection until the release, so a selection
// drags whole.
// As "What uses space" (`spaceView`) it lists folders and files together,
// biggest first, each with a bar against the biggest, under the drive the
// folder is on and how full it is; the header no longer sorts.
// It sits on its own glass or, as `solid` (0..1) rises, the theme's solid
// list surface. In a file picker (`picking`), opening files chooses them
// (`filesChosen`) instead of launching them, a `filter` leaves out the
// files it doesn't match, and without `multiple` one row is selected at a
// time.
FocusScope {
    id: list

    property real solid: 0

    required property string folder
    // Absolute paths to select once the folder is listed.
    property var select: []
    property bool showHidden: false
    property string view: Listing.DETAILS
    readonly property bool details: view === Listing.DETAILS
    property bool spaceView: false
    // The order a header click last asked for, kept while What uses space
    // shows its own.
    property var chosenSort: ({
            key: Protocol.FILE_SORT_NAME,
            descending: false
        })
    // The mounted drive the folder is on, or null.
    readonly property var drive: Space.driveOf(FilesService.drives, folder)

    // A file picker's: {name, globs, mimes}, or null for every file.
    property var filter: null
    property bool picking: false
    property bool multiple: true

    signal folderOpened(string path)
    // In a picker: files opened (double-click, Enter), by name.
    signal filesChosen(var names)
    signal tabRequested(string path)
    // Delete and Shift+Delete with rows selected: plain copies of them
    // (Listing.copy), in the list's order.
    signal trashRequested(var rows)
    signal deleteRequested(var rows)
    // A menu at `at` (in the list) for `rows` (plain copies), none for the
    // folder itself.
    signal menuRequested(var rows, point at)
    // A shortcut for the window: a Commands name, with the selection.
    signal commandRequested(string command, var rows)
    // novad couldn't rename `row`: why.
    signal renameFailed(var row, string error)
    // A drop asks for `action` (a Protocol.TRANSFER_* mode or Drops.TRASH)
    // on `sources` into `folder`.
    signal dropRequested(string action, var sources, string folder)

    // The row whose name is a field, by name, or "".
    property string renaming: ""
    // The items cut, by name in this folder, for their rows to fade.
    readonly property var cutNames: {
        const prefix = Paths.child(folder, "");
        const names = Object.create(null);
        for (const path of FilesClipboard.cut) {
            if (path.startsWith(prefix) && !path.slice(prefix.length).includes("/"))
                names[path.slice(prefix.length)] = true;
        }
        return names;
    }

    // The selection as plain copies, in the list's order.
    function selection(): var {
        return model.selectedRows().map(row => Listing.copy(row));
    }

    // Selects the item `name` alone and turns its name into a field, once
    // it's listed (a new folder comes a moment later).
    function rename(name: string) {
        model.selectNames([name]);
        renaming = name;
        showRenaming();
    }

    function showRenaming() {
        for (let index = 0; index < model.rows.count; index++) {
            if (model.rows.get(index).name === renaming) {
                entryGrid.currentIndex = index;
                entryGrid.positionViewAtIndex(index, GridView.Contain);
                return;
            }
        }
    }

    // The field closed with `typed` ("" for Esc): renames the item unless
    // the name stayed, and keeps it selected under its new name.
    function finishRename(row: var, typed: string) {
        renaming = "";
        entryGrid.forceActiveFocus();
        if (typed.trim() === "" || typed === row.name)
            return;
        const asked = folder;
        FilesService.rename(Paths.child(folder, row.name), typed, (error, name) => {
            if (error)
                list.renameFailed(row, error);
            else if (list.folder === asked)
                list.model.selectNames([name]);
        });
    }

    // Runs `command` (a Commands name) on the selection: the list's own
    // ones here, the rest in the window.
    function run(command: string) {
        const rows = selection();
        if (command === Commands.OPEN)
            open(model.selectedRows());
        else if (command === Commands.SELECT_ALL)
            model.selectAll();
        else if (command === Commands.HIDDEN)
            showHidden = !showHidden;
        else if (command === Commands.TRASH) {
            if (rows.length > 0)
                trashRequested(rows);
        } else if (command === Commands.DELETE_FOR_GOOD) {
            if (rows.length > 0)
                deleteRequested(rows);
        } else if (command === Commands.RENAME) {
            if (rows.length === 1 && !spaceView)
                rename(rows[0].name);
        } else
            commandRequested(command, rows);
    }

    // Narrow beside the details or preview pane: fewer, smaller columns.
    property bool compact: false
    // Modified, Type and Size's widths (a column 0 wide isn't shown), or in
    // What uses space the bar's and Size's.
    readonly property var columnWidths: spaceView ? [Tokens.filesSpaceBarWidth, Tokens.filesColumns[2]] : compact ? Tokens.filesColumnsCompact : Tokens.filesColumns
    readonly property real nameWidth: headerRow.width - columnWidths.reduce((sum, column) => column > 0 ? sum + column + headerRow.spacing : sum, 0)
    // The bar's column has no header, and none of them sort.
    readonly property var spaceColumns: [
        {
            label: "",
            key: ""
        },
        {
            label: qsTr("Size"),
            key: ""
        }
    ]
    readonly property var columns: spaceView ? spaceColumns : detailColumns
    readonly property var detailColumns: [
        {
            label: qsTr("Modified"),
            key: Protocol.FILE_SORT_MODIFIED
        },
        {
            label: qsTr("Type"),
            key: Protocol.FILE_SORT_KIND
        },
        {
            label: qsTr("Size"),
            key: Protocol.FILE_SORT_SIZE
        }
    ]

    // The last press was on a selected row: the click picks, not the press.
    property bool pickOnClick: false

    // The row the list opens at while the rows ease in after a folder's
    // entries came in (they ripple out from it, Arrival), or -1 once they
    // have; and how many came in.
    property int riseFrom: -1
    property int riseCount: 0
    readonly property var risePace: ({
            stagger: Tokens.filesRowStagger,
            wave: Tokens.filesRowWave,
            most: Tokens.filesRowWaveMost
        })
    // The time the Modified column reads against, kept to the minute.
    property real now: Date.now()

    readonly property FolderModel model: FolderModel {
        path: list.folder
        hidden: list.showHidden
        sort: list.spaceView ? Space.BIGGEST_FIRST : list.chosenSort
        filter: list.filter
        onFilling: (current, total) => {
            list.now = Date.now();
            list.riseFrom = Math.max(current, 0);
            list.riseCount = total;
            settle.restart();
        }
        onTotalChanged: {
            if (list.renaming !== "")
                list.showRenaming();
        }
        onReset: current => {
            entryGrid.currentIndex = current;
            if (current >= 0)
                entryGrid.positionViewAtIndex(current, GridView.Contain);
            else
                entryGrid.positionViewAtBeginning();
        }
    }

    // Opens the rows in `picked`: Listing.opening says what that does.
    function open(picked: var) {
        const opening = Listing.opening(picked);
        if (opening.folder !== "")
            folderOpened(Paths.child(folder, opening.folder));
        else if (picking && opening.files.length > 0)
            filesChosen(opening.files);
        if (picking)
            return;
        for (const name of opening.files)
            FilesService.launch(Paths.child(folder, name), "", null);
    }

    // The order and the selected names, for a tab going to the back.
    function tabView(): var {
        return {
            sort: model.sort,
            selected: model.selectedNames()
        };
    }

    // Shows a tab's tabView() (its order; null keeps this one) once its folder
    // is set, with `selected` (names) selected. Shift starts from nothing,
    // not from the last tab's anchor.
    function restoreTab(kept: var, selected: var) {
        if (kept && (kept.sort.key !== model.sort.key || kept.sort.descending !== model.sort.descending))
            model.sort = kept.sort;
        model.selectNames(selected);
        model.anchor = -1;
    }

    // Selects the entry `name` and moves to it: one search result shown
    // in its folder. A dot file shows hidden files first, so it's listed.
    function reveal(name: string) {
        if (name.startsWith("."))
            showHidden = true;
        model.selectNames([name]);
        let found = false;
        for (let index = 0; index < model.rows.count && !found; index++) {
            found = model.rows.get(index).name === name;
            if (found) {
                entryGrid.currentIndex = index;
                entryGrid.positionViewAtIndex(index, GridView.Contain);
            }
        }
        // On a page still to come: the list moves to it when it comes.
        if (!found && model.loaded)
            model._seeking = true;
        entryGrid.forceActiveFocus();
    }

    // Reads the folder again, keeping the selection.
    function refresh() {
        model.refresh();
    }

    // A rename asked for in the folder before doesn't follow to the next.
    onFolderChanged: renaming = ""

    Component.onCompleted: {
        const prefix = Paths.child(folder, "");
        model.selectNames(select.filter(path => path.startsWith(prefix) && !path.slice(prefix.length).includes("/")).map(path => path.slice(prefix.length)));
    }

    Rectangle {
        anchors.fill: parent
        color: Tokens.mix(Tokens.glass, Tokens.filesSolidList, list.solid)
    }

    Timer {
        interval: Tokens.filesClockTick
        running: true
        repeat: true
        onTriggered: list.now = Date.now()
    }
    Timer {
        id: settle
        interval: Tokens.filesRowWave + Tokens.durationRowIn
        onTriggered: list.riseFrom = -1
    }

    component HeaderCell: Item {
        id: cell

        required property string label
        required property string key
        property bool alignRight: false
        readonly property bool sorted: !list.spaceView && list.model.sort.key === key

        height: headerRow.height

        Row {
            anchors.verticalCenter: parent.verticalCenter
            anchors.left: cell.alignRight ? undefined : parent.left
            anchors.right: cell.alignRight ? parent.right : undefined
            anchors.leftMargin: cell.alignRight ? 0 : Tokens.rowInset
            anchors.rightMargin: cell.alignRight ? Tokens.rowInset : 0
            layoutDirection: cell.alignRight ? Qt.RightToLeft : Qt.LeftToRight
            spacing: Tokens.labelNudge * 2

            Text {
                anchors.verticalCenter: parent.verticalCenter
                width: Math.min(implicitWidth, cell.width - Tokens.rowInset - Tokens.iconSizeMini)
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeSmall
                font.weight: Tokens.weightLabel
                color: press.containsMouse ? Tokens.fgDim : Tokens.filesFaint
                elide: Text.ElideRight
                text: cell.label
            }
            LineIcon {
                anchors.verticalCenter: parent.verticalCenter
                visible: cell.sorted
                size: Tokens.iconSizeMini
                color: Tokens.filesFaint
                rotation: list.model.sort.descending ? 0 : 180
                path: Icons.CHEVRON_DOWN
            }
        }

        MouseArea {
            id: press
            anchors.fill: parent
            hoverEnabled: true
            enabled: cell.key !== ""
            onClicked: list.chosenSort = Listing.sorted(list.chosenSort, cell.key)
        }
    }

    SpaceHeader {
        id: spaceHeader
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        drive: list.drive
        shown: list.spaceView && list.drive !== null
    }

    Row {
        id: headerRow
        visible: list.details
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: spaceHeader.bottom
        anchors.margins: Tokens.rowInset
        anchors.topMargin: Tokens.filesRowPadY
        height: Tokens.filesHeaderHeight
        spacing: Tokens.rowInset

        HeaderCell {
            width: list.nameWidth
            label: qsTr("Name")
            key: Protocol.FILE_SORT_NAME
        }
        Repeater {
            model: list.columns

            HeaderCell {
                required property var modelData
                required property int index

                width: list.columnWidths[index]
                visible: width > 0
                label: modelData.label
                key: modelData.key
                alignRight: index === list.columns.length - 1
            }
        }
    }

    // Details' rows are a grid one column wide, so both views share the
    // selection, the keys and the scrolling.
    GridView {
        id: entryGrid
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: list.details ? headerRow.bottom : parent.top
        anchors.bottom: countLine.top
        anchors.leftMargin: Tokens.rowInset
        anchors.rightMargin: Tokens.rowInset
        anchors.topMargin: list.details ? 0 : Tokens.filesRowPadY
        clip: true
        focus: true
        boundsBehavior: Flickable.StopAtBounds
        keyNavigationEnabled: false
        model: list.model.rows
        cellWidth: list.details ? width : Tokens.filesTileWidth + Tokens.filesTileGap
        cellHeight: list.details ? Tokens.filesRowHeight + Tokens.dividerWidth : Tokens.filesTileHeight + Tokens.filesTileGap
        delegate: list.details ? rowDelegate : tileDelegate

        readonly property int columns: Math.max(1, Math.floor(width / Math.max(cellWidth, 1)))
        readonly property int lines: Math.max(1, Math.floor(height / cellHeight))
        readonly property int page: columns * lines

        // The other view keeps the current entry in sight.
        onDelegateChanged: Qt.callLater(() => {
            if (currentIndex >= 0)
                positionViewAtIndex(currentIndex, GridView.Contain);
            else
                positionViewAtBeginning();
        })

        // Moves to entry `index` and selects what a click there would.
        function go(index: int, extend: bool) {
            entryGrid.currentIndex = index;
            entryGrid.positionViewAtIndex(index, GridView.Contain);
            list.model.select(Listing.pick(list.model.anchor, index, false, extend && list.multiple));
        }

        // A press on entry `index` (`selected` already) with `modifiers`
        // held. A selected entry pressed without Shift may start dragging
        // the whole selection: what the press picks waits for the click,
        // Ctrl's toggle too.
        function press(index: int, selected: bool, modifiers: int) {
            entryGrid.forceActiveFocus();
            entryGrid.currentIndex = index;
            list.pickOnClick = selected && (modifiers & Qt.ShiftModifier) === 0;
            if (!list.pickOnClick)
                list.model.select(Listing.pick(list.model.anchor, index, list.multiple && (modifiers & Qt.ControlModifier) !== 0, list.multiple && (modifiers & Qt.ShiftModifier) !== 0));
        }

        // The press on entry `index` ended as a click.
        function click(index: int, modifiers: int) {
            if (list.pickOnClick)
                list.model.select(Listing.pick(list.model.anchor, index, list.multiple && (modifiers & Qt.ControlModifier) !== 0, false));
        }

        // Entry `index` was pressed and moved: the selection drags out.
        function drag(index: int) {
            const grabbed = list.model.rows.get(index);
            fileDrag.start(list.model.selectedRows().map(picked => Paths.child(list.folder, picked.name)), Listing.icon(grabbed), grabbed.dir ? Tokens.filesFolder : Tokens.fgDim);
        }

        // How long entry `index` waits before easing in while a folder's
        // entries have just come in (Arrival), or -1. The wave ripples out
        // from where the list opened, as far as a screen reaches either way.
        function riseWait(index: int): int {
            const reach = Math.max(lines, columns);
            return Arrival.wait(Arrival.order(index, list.riseFrom, reach, columns), Math.min(list.riseCount, reach + 1), list.riseCount, list.risePace);
        }

        MouseArea {
            // A click below or beside the entries clears the selection; a
            // right-click there asks for the folder's menu.
            z: -1
            width: entryGrid.width
            height: Math.max(entryGrid.height, entryGrid.contentHeight)
            acceptedButtons: Qt.LeftButton | Qt.RightButton
            onPressed: press => {
                entryGrid.forceActiveFocus();
                list.model.selectNames([]);
                if (press.button === Qt.RightButton)
                    list.menuRequested([], mapToItem(list, press.x, press.y));
            }
        }

        Component {
            id: rowDelegate

            FileRow {
                id: row

                width: entryGrid.width
                nameWidth: list.nameWidth
                columnWidths: list.columnWidths
                bar: list.spaceView ? Space.share(row.size, list.model.firstSize) : -1
                selected: list.model.revision >= 0 && list.model.isSelected(name)
                riseWait: entryGrid.riseWait(index)
                now: list.now
                cut: list.cutNames[row.name] === true
                renaming: list.renaming !== "" && list.renaming === row.name
                dropTarget: row.dir && drops.action !== Drops.NONE && drops.hovered?.folder === Paths.child(list.folder, row.name)

                onMenuAsked: at => {
                    entryGrid.forceActiveFocus();
                    entryGrid.currentIndex = row.index;
                    if (!list.model.isSelected(row.name))
                        list.model.selectNames([row.name]);
                    list.menuRequested(list.selection(), row.mapToItem(list, at.x, at.y));
                }
                onRenameDone: typed => list.finishRename(Listing.copy(list.model.rows.get(row.index)), typed)
                onPressed: modifiers => entryGrid.press(row.index, row.selected, modifiers)
                onClicked: modifiers => entryGrid.click(row.index, modifiers)
                onDragRequested: entryGrid.drag(row.index)
                onOpened: list.open([list.model.rows.get(row.index)])
                onMiddleClicked: {
                    if (row.dir)
                        list.tabRequested(Paths.child(list.folder, row.name));
                }
            }
        }
        Component {
            id: tileDelegate

            IconTile {
                id: tile

                folder: list.model.path
                thumbnails: FilesService
                selected: list.model.revision >= 0 && list.model.isSelected(name)
                riseWait: entryGrid.riseWait(index)

                onPressed: modifiers => entryGrid.press(tile.index, tile.selected, modifiers)
                onOpened: list.open([list.model.rows.get(tile.index)])
            }
        }

        NameTyping {
            id: typing
        }

        Keys.onPressed: event => {
            const command = Shortcuts.action(Shortcuts.LIST, event.key, event.modifiers);
            const sideways = !list.details && (event.key === Qt.Key_Left || event.key === Qt.Key_Right) && (event.modifiers & ~Qt.ShiftModifier & ~Qt.KeypadModifier) === 0;
            if (command === Commands.MOVE || sideways) {
                if (entryGrid.count > 0)
                    entryGrid.go(Listing.moved(event.key, entryGrid.currentIndex, entryGrid.count, entryGrid.page, list.details ? 1 : entryGrid.columns), (event.modifiers & Qt.ShiftModifier) !== 0);
            } else if (command === Commands.SELECT_ALL && !list.multiple) {
                // One file at a time in this picker.
            } else if (command !== "")
                list.run(command);
            else
                event.accepted = typing.press(event.text, event.modifiers, list.model.rows, entryGrid.currentIndex, index => entryGrid.go(index, false));
        }
    }

    DropTarget {
        id: drops
        anchors.fill: entryGrid
        service: FilesService
        folderAt: (x, y) => {
            const index = entryGrid.indexAt(x + entryGrid.contentX, y + entryGrid.contentY);
            const row = index >= 0 ? list.model.rows.get(index) : null;
            return row?.dir ? Drops.place(Paths.child(list.folder, row.name), true) : Drops.place(list.folder, false);
        }
        onDropRequested: (action, sources, folder) => list.dropRequested(action, sources, folder)
        onSpringOpened: folder => list.folderOpened(folder)
    }
    FileDrag {
        id: fileDrag
    }

    Text {
        id: countLine
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        leftPadding: 2 * Tokens.rowInset
        rightPadding: 2 * Tokens.rowInset
        height: Tokens.filesHeaderHeight
        verticalAlignment: Text.AlignVCenter
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeSmall
        color: Tokens.filesFaint
        elide: Text.ElideRight
        textFormat: Text.PlainText
        text: list.model.error !== "" ? list.model.error : list.model.loaded ? Listing.items(list.model.total) : ""
    }
}
