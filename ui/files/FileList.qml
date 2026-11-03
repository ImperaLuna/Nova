pragma ComponentBehavior: Bound

import QtQuick
import "service"
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol
import "drops.js" as Drops
import "listing.js" as Listing
import "paths.js" as Paths
import "shortcuts.js" as Shortcuts

// Nova Files' file list in Details: the column header (Name, then
// Modified, Type and Size at their widths; a click sorts by that column,
// again turns the order round) over the folder's rows from novad
// (FolderModel), and the item count under them. A click selects a row,
// Ctrl adds or drops one, Shift selects a run. Its keys are the table's
// LIST entries (shortcuts.js): the arrows, Page Up/Down, Home and End move
// the selection (Shift extends it), Ctrl+A selects all, Ctrl+H shows or
// hides dot files, Backspace goes back and Ctrl+Z and Ctrl+Y undo and redo
// (`actionRequested`), and typing a name goes to it (NameTyping). A double-click or Enter opens: a folder is
// gone into (`folderOpened`), files open in their default app. Delete
// asks to move the selection to the trash (`trashRequested`),
// Shift+Delete to delete it for good (`deleteRequested`), Alt+Enter asks
// for its Properties, or the folder's with nothing selected
// (`propertiesRequested`). F5 (the
// window's) reads the folder again (`refresh`).
// Rows drag out (the selection, FileDrag) and files dragged in drop on the
// folder row under the pointer, or else the folder shown
// (`dropRequested`); resting on a folder row opens it. Pressing a
// selected row keeps the selection until the release, so a selection
// drags whole.
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
    signal propertiesRequested(var rows)
    // A table action that is the window's (Backspace's "back", Ctrl+Z's
    // "undo", Ctrl+Y's "redo").
    signal actionRequested(string action)
    // A drop asks for `action` (a Protocol.TRANSFER_* mode or Drops.TRASH)
    // on `sources` into `folder`.
    signal dropRequested(string action, var sources, string folder)

    readonly property real nameWidth: headerRow.width - Tokens.filesColumns.reduce((sum, column) => sum + column + headerRow.spacing, 0)
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

    // The last press was on a selected row: the click picks, not the press.
    property bool pickOnClick: false

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

    // Reads the folder again, keeping the selection.
    function refresh() {
        model.refresh();
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

                width: Tokens.filesColumns[index]
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
            // A click below the rows clears the selection.
            z: -1
            width: entryList.width
            height: Math.max(entryList.height, entryList.contentHeight)
            onPressed: {
                entryList.forceActiveFocus();
                list.model.selectNames([]);
            }
        }

        delegate: FileRow {
            id: row

            width: entryList.width
            nameWidth: list.nameWidth
            selected: list.model.revision >= 0 && list.model.isSelected(name)
            riseOrder: list.riseFrom >= 0 ? Math.min(Math.abs(index - list.riseFrom), entryList.page) : -1
            now: list.now

            dropTarget: row.dir && drops.action !== Drops.NONE && drops.hovered?.folder === Paths.child(list.folder, row.name)

            onPressed: modifiers => {
                entryList.forceActiveFocus();
                entryList.currentIndex = row.index;
                // A selected row pressed without Shift may start dragging
                // the whole selection: what the press picks waits for the
                // click (below), Ctrl's toggle too.
                list.pickOnClick = row.selected && (modifiers & Qt.ShiftModifier) === 0;
                if (!list.pickOnClick)
                    list.model.select(Listing.pick(list.model.anchor, row.index, (modifiers & Qt.ControlModifier) !== 0, (modifiers & Qt.ShiftModifier) !== 0));
            }
            onClicked: modifiers => {
                if (list.pickOnClick)
                    list.model.select(Listing.pick(list.model.anchor, row.index, (modifiers & Qt.ControlModifier) !== 0, false));
            }
            onDragRequested: {
                const grabbed = list.model.rows.get(row.index);
                fileDrag.start(list.model.selectedRows().map(picked => Paths.child(list.folder, picked.name)), Listing.icon(grabbed), grabbed.dir ? Tokens.filesFolder : Tokens.fgDim);
            }
            onOpened: list.open([list.model.rows.get(row.index)])
        }

        NameTyping {
            id: typing
        }

        Keys.onPressed: event => {
            const picked = () => list.model.selectedRows().map(row => Listing.copy(row));
            const action = Shortcuts.action(Shortcuts.LIST, event.key, event.modifiers);
            switch (action) {
            case "hidden":
                list.showHidden = !list.showHidden;
                return;
            case "selectAll":
                list.model.selectAll();
                return;
            case "open":
                list.open(list.model.selectedRows());
                return;
            case "back":
            case "undo":
            case "redo":
                list.actionRequested(action);
                return;
            case "properties":
                list.propertiesRequested(picked());
                return;
            case "trash":
                if (list.model.selectedCount > 0)
                    list.trashRequested(picked());
                return;
            case "deleteForGood":
                if (list.model.selectedCount > 0)
                    list.deleteRequested(picked());
                return;
            case "move":
                if (entryList.count > 0)
                    entryList.go(Listing.moved(event.key, entryList.currentIndex, entryList.count, entryList.page), (event.modifiers & Qt.ShiftModifier) !== 0);
                return;
            }
            event.accepted = typing.press(event.text, event.modifiers, list.model.rows, entryList.currentIndex, index => entryList.go(index, false));
        }
    }

    DropTarget {
        id: drops
        anchors.fill: entryList
        service: FilesService
        folderAt: (x, y) => {
            const index = entryList.indexAt(x + entryList.contentX, y + entryList.contentY);
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
