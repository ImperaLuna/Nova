pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../theme"
import "listing.js" as Listing
import "trash.js" as Trash

// The trash's list, where a window's file list goes while it shows the
// trash: the column header (Name, then Original location, Date deleted and
// Size, after Windows' Recycle Bin) over every trashed item, the last
// trashed first, and the count at the foot. Click, Ctrl and Shift pick
// items, the arrow keys move, Ctrl+A picks all, Delete asks to delete the
// picked ones for good. A trashed item doesn't open. Like the file list,
// it sits on its own glass or, as `solid` (0..1) rises, the theme's solid
// list surface.
Item {
    id: list

    required property TrashModel model
    property real solid: 0
    readonly property var columns: [qsTr("Original location"), qsTr("Date deleted"), qsTr("Size")]
    readonly property real nameWidth: headerRow.width - Tokens.filesTrashColumns.reduce((sum, column) => sum + column + headerRow.spacing, 0)
    // When the Date deleted column was last worked out.
    property real now: Date.now()
    readonly property string home: Quickshell.env("HOME") ?? ""

    // Delete was pressed with items picked.
    signal eraseRequested

    function focusList() {
        rows.forceActiveFocus();
    }

    component HeaderLabel: Text {
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeSmall
        font.weight: Tokens.weightLabel
        color: Tokens.filesFaint
        elide: Text.ElideRight
    }

    Rectangle {
        anchors.fill: parent
        color: Tokens.mix(Tokens.glass, Tokens.filesSolidList, list.solid)
    }

    Timer {
        interval: Tokens.filesClockTick
        running: list.visible
        repeat: true
        onTriggered: list.now = Date.now()
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

        HeaderLabel {
            anchors.verticalCenter: parent.verticalCenter
            width: list.nameWidth
            leftPadding: Tokens.rowInset
            text: qsTr("Name")
        }
        Repeater {
            model: list.columns

            HeaderLabel {
                required property string modelData
                required property int index

                anchors.verticalCenter: parent.verticalCenter
                width: Tokens.filesTrashColumns[index]
                horizontalAlignment: index === list.columns.length - 1 ? Text.AlignRight : Text.AlignLeft
                rightPadding: index === list.columns.length - 1 ? Tokens.rowInset : 0
                text: modelData
            }
        }
    }

    ListView {
        id: rows
        anchors.left: headerRow.left
        anchors.right: headerRow.right
        anchors.top: headerRow.bottom
        anchors.bottom: foot.top
        clip: true
        spacing: Tokens.dividerWidth
        boundsBehavior: Flickable.StopAtBounds
        focus: true
        keyNavigationEnabled: false
        model: list.model.rows

        readonly property int page: Math.max(1, Math.floor(height / (Tokens.filesRowHeight + spacing)))

        // Moves to row `index` and selects what a click there would.
        function go(index: int, extend: bool) {
            rows.currentIndex = index;
            rows.positionViewAtIndex(index, ListView.Contain);
            list.model.select(Listing.pick(list.model.anchor, index, false, extend));
        }

        MouseArea {
            // A click below the rows clears the selection.
            z: -1
            width: rows.width
            height: Math.max(rows.height, rows.contentHeight)
            onPressed: {
                rows.forceActiveFocus();
                list.model.clearSelection();
            }
        }

        delegate: TrashRow {
            id: row

            required property string trashId

            width: rows.width
            nameWidth: list.nameWidth
            now: list.now
            home: list.home
            selected: list.model.revision >= 0 && list.model.isSelected(trashId)
            onPressed: modifiers => {
                rows.forceActiveFocus();
                rows.currentIndex = row.index;
                list.model.select(Listing.pick(list.model.anchor, row.index, (modifiers & Qt.ControlModifier) !== 0, (modifiers & Qt.ShiftModifier) !== 0));
            }
        }

        Keys.onPressed: event => {
            if (event.key === Qt.Key_A && (event.modifiers & Qt.ControlModifier)) {
                list.model.selectAll();
                event.accepted = true;
            } else if (event.key === Qt.Key_Delete && list.model.selectedCount > 0) {
                list.eraseRequested();
                event.accepted = true;
            } else if (Listing.MOVES.includes(event.key) && rows.count > 0) {
                rows.go(Listing.moved(event.key, rows.currentIndex, rows.count, rows.page), (event.modifiers & Qt.ShiftModifier) !== 0);
                event.accepted = true;
            }
        }
    }

    Text {
        id: foot
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
        text: list.model.loaded ? Trash.footer(list.model.selectedCount, list.model.rows.count) : ""
    }
}
