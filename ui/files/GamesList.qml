pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "listing.js" as Listing

// The Games place's list, where a window's file list goes while it shows
// the games: the column header (Name, Location, Size) over every
// installed game in name order, and the count at the foot. A click picks
// a game, the arrow keys move, a double click or Enter opens its install
// folder, a right click asks for its menu. Like the file list, it sits on
// its own glass or, as `solid` (0..1) rises, the theme's solid list
// surface.
Item {
    id: list

    required property GamesModel model
    property real solid: 0
    readonly property var columns: [qsTr("Location"), qsTr("Size")]
    readonly property real nameWidth: headerRow.width - Tokens.filesGamesColumns.reduce((sum, column) => sum + column + headerRow.spacing, 0)
    // Home, for the Location column.
    required property string home

    // The game at row `index` is to be opened: its install folder.
    signal opened(int index)
    // The menu of the game at row `index` was asked for at `at` (in the
    // window).
    signal menuRequested(int index, point at)

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
                width: Tokens.filesGamesColumns[index]
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

        function go(index: int) {
            rows.positionViewAtIndex(index, ListView.Contain);
            list.model.pick(index);
        }

        MouseArea {
            // A click below the rows picks nothing.
            z: -1
            width: rows.width
            height: Math.max(rows.height, rows.contentHeight)
            onPressed: {
                rows.forceActiveFocus();
                list.model.pick(-1);
            }
        }

        delegate: GameRow {
            id: row

            width: rows.width
            nameWidth: list.nameWidth
            home: list.home
            selected: list.model.picked === row.index
            onPressed: {
                rows.forceActiveFocus();
                list.model.pick(row.index);
            }
            onOpened: list.opened(row.index)
            onMenuRequested: at => list.menuRequested(row.index, at)
        }

        Keys.onPressed: event => {
            if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter) && list.model.picked >= 0) {
                list.opened(list.model.picked);
                event.accepted = true;
            } else if (Listing.MOVES.includes(event.key) && rows.count > 0) {
                rows.go(Listing.moved(event.key, list.model.picked, rows.count, rows.page));
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
        text: list.model.loaded ? Listing.items(list.model.rows.count) : ""
    }
}
