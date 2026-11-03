pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "paths.js" as Paths
import "search.js" as Search

// Nova Files' search results, in the file list's place while something is
// typed in the search field: where to search (the folder shown, the
// default, or everywhere: Home and the drives) as two round chips, the
// column header (Name, Location, Size; results stay best first), the
// results from `model` (a SearchModel) and their count.
// A click selects a result, the arrows, Page Up/Down, Home and End move
// the selection; a double-click or Enter opens it (`opened`), a click on
// where it lives shows it in its folder (`locationOpened`). Esc goes back
// to the folder (`closed`).
FocusScope {
    id: results

    required property SearchModel model
    // The folder shown, the first scope.
    required property string folder
    property bool everywhere: false
    // The folder shown can be searched; elsewhere only Everywhere shows.
    property bool folderIndexed: true
    // Home, which result locations are shown from.
    property string home: ""
    property real solid: 0

    // A Search.row to open, or to show in its folder.
    signal opened(var row)
    signal locationOpened(var row)
    signal closed

    readonly property real nameWidth: headerRow.width - Tokens.filesSearchLocationWidth - Tokens.filesColumns[2] - 2 * headerRow.spacing

    // Puts the keyboard on the results, the first one selected.
    function focusResults() {
        if (resultList.currentIndex < 0 && resultList.count > 0)
            resultList.currentIndex = 0;
        resultList.forceActiveFocus();
    }

    Rectangle {
        anchors.fill: parent
        color: Tokens.mix(Tokens.glass, Tokens.filesSolidList, results.solid)
    }

    component ScopeChip: Rectangle {
        id: chip

        required property string label
        property string icon: ""
        required property bool picked
        signal chosen

        height: Tokens.filesScopeHeight
        width: chipRow.implicitWidth + 2 * Tokens.chipPad
        radius: height / 2
        color: picked ? Tokens.selected : chipHover.hovered ? Tokens.hover : Tokens.alpha(Tokens.surfaceRaised, Tokens.raisedOpacity)

        Behavior on color {
            ColorAnimation {
                duration: Tokens.durationFast
            }
        }

        Row {
            id: chipRow
            anchors.centerIn: parent
            spacing: Tokens.iconGap

            LineIcon {
                anchors.verticalCenter: parent.verticalCenter
                visible: chip.icon !== ""
                size: Tokens.iconSizeMini
                color: chip.picked ? Tokens.fg : Tokens.fgDim
                path: chip.icon
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeSmall
                font.weight: chip.picked ? Tokens.weightLabel : Tokens.weightBody
                color: chip.picked ? Tokens.fg : Tokens.fgDim
                textFormat: Text.PlainText
                text: chip.label
            }
        }
        HoverHandler {
            id: chipHover
        }
        TapHandler {
            onTapped: chip.chosen()
        }
    }

    Row {
        id: scopes
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.leftMargin: 2 * Tokens.rowInset
        anchors.topMargin: Tokens.filesRowPadY + Tokens.filesScopeGap
        spacing: Tokens.filesScopeGap

        ScopeChip {
            icon: Icons.FOLDER
            label: Paths.folderName(results.folder)
            visible: results.folderIndexed
            picked: !results.everywhere
            onChosen: results.everywhere = false
        }
        ScopeChip {
            icon: Icons.SEARCH
            label: qsTr("Everywhere")
            picked: results.everywhere || !results.folderIndexed
            onChosen: results.everywhere = true
        }
    }

    component HeaderCell: Text {
        anchors.verticalCenter: parent?.verticalCenter
        leftPadding: Tokens.rowInset
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeSmall
        font.weight: Tokens.weightLabel
        color: Tokens.filesFaint
        elide: Text.ElideRight
    }

    Row {
        id: headerRow
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: scopes.bottom
        anchors.margins: Tokens.rowInset
        anchors.topMargin: Tokens.filesScopeGap
        height: Tokens.filesHeaderHeight
        spacing: Tokens.rowInset

        HeaderCell {
            width: results.nameWidth
            text: qsTr("Name")
        }
        HeaderCell {
            width: Tokens.filesSearchLocationWidth
            text: qsTr("Location")
        }
        HeaderCell {
            width: Tokens.filesColumns[2]
            rightPadding: Tokens.rowInset
            horizontalAlignment: Text.AlignRight
            text: qsTr("Size")
        }
    }

    ListView {
        id: resultList
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
        currentIndex: -1
        model: results.model.rows

        readonly property int page: Math.max(1, Math.floor(height / (Tokens.filesRowHeight + spacing)))

        function go(index: int) {
            if (count === 0)
                return;
            currentIndex = Math.max(0, Math.min(index, count - 1));
            positionViewAtIndex(currentIndex, ListView.Contain);
        }

        // New results: nothing picked until a click or a key.
        onModelChanged: currentIndex = activeFocus && count > 0 ? 0 : -1

        delegate: SearchRow {
            id: row

            width: resultList.width
            nameWidth: results.nameWidth
            selected: resultList.currentIndex === index
            home: results.home

            onPressed: {
                resultList.forceActiveFocus();
                resultList.currentIndex = row.index;
            }
            onOpened: results.opened(row.modelData)
            onLocationOpened: results.locationOpened(row.modelData)
        }

        Keys.onPressed: event => {
            const row = results.model.rows[currentIndex];
            if (event.key === Qt.Key_Escape)
                results.closed();
            else if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter) && row)
                results.opened(row);
            else if (event.key === Qt.Key_Down)
                go(currentIndex + 1);
            else if (event.key === Qt.Key_Up)
                go(currentIndex - 1);
            else if (event.key === Qt.Key_PageDown)
                go(currentIndex + page);
            else if (event.key === Qt.Key_PageUp)
                go(currentIndex - page);
            else if (event.key === Qt.Key_Home)
                go(0);
            else if (event.key === Qt.Key_End)
                go(count - 1);
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
        text: results.model.answered ? Search.count(results.model.ready, results.model.total, results.model.rows.length) : ""
    }
}
