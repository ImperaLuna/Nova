pragma ComponentBehavior: Bound

import QtQuick
import "service"
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol
import "arrival.js" as Arrival
import "listing.js" as Listing
import "paths.js" as Paths
import "space.js" as Space

// Nova Files' file list: the folder's entries from novad (FolderModel) in
// one of two views (`view`, a Listing view), and the item count under
// them. Details shows rows under the column header (Name, then Modified,
// Type and Size at their widths; a click sorts by that column, again turns
// the order round); Icons shows tiles, line after line, in the same order.
// A click selects an entry, Ctrl adds or drops one, Shift selects a run;
// the arrows (Left and Right too in Icons), Page Up/Down, Home and End
// move the selection (Shift extends it), Ctrl+A selects all, Ctrl+H shows
// or hides dot files. A double-click or Enter opens: a
// folder is gone into (`folderOpened`), files open in their default app.
// Delete asks to move the selection to the trash (`trashRequested`),
// Shift+Delete to delete it for good (`deleteRequested`); Alt+Enter asks
// for its Properties, or the folder's with nothing selected
// (`propertiesRequested`). A middle click on a folder asks for it in a new
// tab (`tabRequested`). view() and show() hand its order and selection to
// the window's tabs and back.
// As "What uses space" (`spaceView`) it lists folders and files together,
// biggest first, each with a bar against the biggest, under the drive the
// folder is on and how full it is; the header no longer sorts.
// It sits on its own glass or, as `solid` (0..1) rises, the theme's solid
// list surface.
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

    signal folderOpened(string path)
    signal tabRequested(string path)
    // Delete and Shift+Delete with rows selected: plain copies of them
    // (Listing.copy), in the list's order.
    signal trashRequested(var rows)
    signal deleteRequested(var rows)
    signal propertiesRequested(var rows)

    readonly property var columnWidths: spaceView ? [Tokens.filesSpaceBarWidth, Tokens.filesColumns[2]] : Tokens.filesColumns
    readonly property real nameWidth: headerRow.width - columnWidths.reduce((sum, column) => sum + column + headerRow.spacing, 0)
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
        onFilling: (current, total) => {
            list.now = Date.now();
            list.riseFrom = Math.max(current, 0);
            list.riseCount = total;
            settle.restart();
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
        for (const name of opening.files)
            FilesService.launch(Paths.child(folder, name), "", null);
    }

    // The order and the selected names, for a tab going to the back.
    function view(): var {
        return {
            sort: model.sort,
            selected: model.selectedNames()
        };
    }

    // Shows a tab's view() (its order; null keeps this one) once its folder
    // is set, with `selected` (names) selected. Shift starts from nothing,
    // not from the last tab's anchor.
    function show(kept: var, selected: var) {
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
            list.model.select(Listing.pick(list.model.anchor, index, false, extend));
        }

        // A press on entry `index` with `modifiers` held.
        function press(index: int, modifiers: int) {
            entryGrid.forceActiveFocus();
            entryGrid.currentIndex = index;
            list.model.select(Listing.pick(list.model.anchor, index, (modifiers & Qt.ControlModifier) !== 0, (modifiers & Qt.ShiftModifier) !== 0));
        }

        // How long entry `index` waits before easing in while a folder's
        // entries have just come in (Arrival), or -1. The wave ripples out
        // from where the list opened, as far as a screen reaches either way.
        function riseWait(index: int): int {
            const reach = Math.max(lines, columns);
            return Arrival.wait(Arrival.order(index, list.riseFrom, reach, columns), Math.min(list.riseCount, reach + 1), list.riseCount, list.risePace);
        }

        MouseArea {
            // A click below or beside the entries clears the selection.
            z: -1
            width: entryGrid.width
            height: Math.max(entryGrid.height, entryGrid.contentHeight)
            onPressed: {
                entryGrid.forceActiveFocus();
                list.model.selectNames([]);
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

                onPressed: modifiers => entryGrid.press(row.index, modifiers)
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

                onPressed: modifiers => entryGrid.press(tile.index, modifiers)
                onOpened: list.open([list.model.rows.get(tile.index)])
            }
        }

        Keys.onPressed: event => {
            const ctrl = (event.modifiers & Qt.ControlModifier) !== 0;
            const moves = list.details ? Listing.MOVES : Listing.GRID_MOVES;
            if (event.key === Qt.Key_H && ctrl)
                list.showHidden = !list.showHidden;
            else if (event.key === Qt.Key_A && ctrl)
                list.model.selectAll();
            else if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter) && (event.modifiers & Qt.AltModifier) !== 0)
                list.propertiesRequested(list.model.selectedRows().map(row => Listing.copy(row)));
            else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
                list.open(list.model.selectedRows());
            else if (event.key === Qt.Key_Delete && list.model.selectedCount > 0) {
                const rows = list.model.selectedRows().map(row => Listing.copy(row));
                if ((event.modifiers & Qt.ShiftModifier) !== 0)
                    list.deleteRequested(rows);
                else
                    list.trashRequested(rows);
            } else if (moves.includes(event.key) && entryGrid.count > 0)
                entryGrid.go(Listing.moved(event.key, entryGrid.currentIndex, entryGrid.count, entryGrid.page, list.details ? 1 : entryGrid.columns), (event.modifiers & Qt.ShiftModifier) !== 0);
            else
                event.accepted = false;
        }
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
