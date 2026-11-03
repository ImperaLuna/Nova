pragma ComponentBehavior: Bound

import QtQuick
import "service"
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol
import "listing.js" as Listing
import "paths.js" as Paths
import "commands.js" as Commands

// Nova Files' file list in Details: the column header (Name, then
// Modified, Type and Size at their widths; a click sorts by that column,
// again turns the order round) over the folder's rows from novad
// (FolderModel), and the item count under them. A click selects a row,
// Ctrl adds or drops one, Shift selects a run; the arrows, Page Up/Down,
// Home and End move the selection (Shift extends it), Ctrl+A selects all,
// Ctrl+H shows or hides dot files. A double-click or Enter opens: a
// folder is gone into (`folderOpened`), files open in their default app.
// Delete asks to move the selection to the trash (`trashRequested`),
// Shift+Delete to delete it for good (`deleteRequested`). A right-click
// asks for a menu (`menuRequested`): on an item, for the selection,
// picking the item first unless it's already picked; on the empty list,
// for the folder. The other shortcuts (commands.js) go to the window as
// `commandRequested`. `rename(name)` turns that row's name into a field
// (F2, or a new folder), renamed through novad when it closes. Cut items
// read faded.
// It sits on its own glass or, as `solid` (0..1) rises, the theme's solid
// list surface.
FocusScope {
    id: list

    property real solid: 0

    required property string folder
    // Absolute paths to select once the folder is listed.
    property var select: []
    property bool showHidden: false

    signal folderOpened(string path)
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
                entryList.currentIndex = index;
                entryList.positionViewAtIndex(index, ListView.Contain);
                return;
            }
        }
    }

    // The field closed with `typed` ("" for Esc): renames the item unless
    // the name stayed, and keeps it selected under its new name.
    function finishRename(row: var, typed: string) {
        renaming = "";
        entryList.forceActiveFocus();
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
        } else if (command === Commands.RENAME) {
            if (rows.length === 1)
                rename(rows[0].name);
        } else
            commandRequested(command, rows);
    }

    // Narrow beside the details pane: fewer, smaller columns.
    property bool compact: false
    // Modified, Type and Size's widths; a column 0 wide isn't shown.
    readonly property var columnWidths: compact ? Tokens.filesColumnsCompact : Tokens.filesColumns
    readonly property real nameWidth: headerRow.width - columnWidths.reduce((sum, column) => column > 0 ? sum + column + headerRow.spacing : sum, 0)
    readonly property var columns: [
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
    // entries came in (they ripple out from it), or -1 once they have.
    property int riseFrom: -1
    // The time the Modified column reads against, kept to the minute.
    property real now: Date.now()

    readonly property FolderModel model: FolderModel {
        path: list.folder
        hidden: list.showHidden
        onFilling: current => {
            list.now = Date.now();
            list.riseFrom = Math.max(current, 0);
            settle.restart();
        }
        onTotalChanged: {
            if (list.renaming !== "")
                list.showRenaming();
        }
        onReset: current => {
            entryList.currentIndex = current;
            if (current >= 0)
                entryList.positionViewAtIndex(current, ListView.Contain);
            else
                entryList.positionViewAtBeginning();
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
        interval: Tokens.durationRowIn + Tokens.durationPanel
        onTriggered: list.riseFrom = -1
    }

    component HeaderCell: Item {
        id: cell

        required property string label
        required property string key
        property bool alignRight: false
        readonly property bool sorted: list.model.sort.key === key

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
            onClicked: list.model.sort = Listing.sorted(list.model.sort, cell.key)
        }
    }

    Row {
        id: headerRow
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
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

    ListView {
        id: entryList
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: headerRow.bottom
        anchors.bottom: countLine.top
        anchors.leftMargin: Tokens.rowInset
        anchors.rightMargin: Tokens.rowInset
        clip: true
        focus: true
        spacing: Tokens.dividerWidth
        boundsBehavior: Flickable.StopAtBounds
        keyNavigationEnabled: false
        model: list.model.rows

        readonly property int page: Math.max(1, Math.floor(height / (Tokens.filesRowHeight + spacing)))

        // Moves to row `index` and selects what a click there would.
        function go(index: int, extend: bool) {
            entryList.currentIndex = index;
            entryList.positionViewAtIndex(index, ListView.Contain);
            list.model.select(Listing.pick(list.model.anchor, index, false, extend));
        }

        MouseArea {
            // A click below the rows clears the selection; a right-click
            // there asks for the folder's menu.
            z: -1
            width: entryList.width
            height: Math.max(entryList.height, entryList.contentHeight)
            acceptedButtons: Qt.LeftButton | Qt.RightButton
            onPressed: press => {
                entryList.forceActiveFocus();
                list.model.selectNames([]);
                if (press.button === Qt.RightButton)
                    list.menuRequested([], mapToItem(list, press.x, press.y));
            }
        }

        delegate: FileRow {
            id: row

            width: entryList.width
            nameWidth: list.nameWidth
            columnWidths: list.columnWidths
            selected: list.model.revision >= 0 && list.model.isSelected(name)
            riseOrder: list.riseFrom >= 0 ? Math.min(Math.abs(index - list.riseFrom), entryList.page) : -1
            now: list.now
            cut: list.cutNames[row.name] === true
            renaming: list.renaming !== "" && list.renaming === row.name

            onMenuAsked: at => {
                entryList.forceActiveFocus();
                entryList.currentIndex = row.index;
                if (!list.model.isSelected(row.name))
                    list.model.selectNames([row.name]);
                list.menuRequested(list.selection(), row.mapToItem(list, at.x, at.y));
            }
            onRenameDone: typed => list.finishRename(Listing.copy(list.model.rows.get(row.index)), typed)

            onPressed: modifiers => {
                entryList.forceActiveFocus();
                entryList.currentIndex = row.index;
                list.model.select(Listing.pick(list.model.anchor, row.index, (modifiers & Qt.ControlModifier) !== 0, (modifiers & Qt.ShiftModifier) !== 0));
            }
            onOpened: list.open([list.model.rows.get(row.index)])
        }

        Keys.onPressed: event => {
            const command = Commands.forKey(event.key, event.modifiers);
            if (command !== "")
                list.run(command);
            else if (event.key === Qt.Key_Delete && (event.modifiers & Qt.ShiftModifier) !== 0) {
                if (list.model.selectedCount > 0)
                    list.deleteRequested(list.selection());
            } else if (Listing.MOVES.includes(event.key) && entryList.count > 0)
                entryList.go(Listing.moved(event.key, entryList.currentIndex, entryList.count, entryList.page), (event.modifiers & Qt.ShiftModifier) !== 0);
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
