import QtQuick
import "../theme"
import "listing.js" as Listing
import "paths.js" as Paths

// A row of Nova Files' Details list: the entry's icon and name, then its
// Modified, Type and Size at the header's column widths. Hovered rows
// lighten, selected ones fill, a folder a drag would drop into washes in
// the accent; dot files read fainter. A press that moves far enough asks
// to drag (`dragRequested`); one that doesn't is a click. When a folder's
// entries have just come in (`riseOrder`), it eases up into place after
// the rows nearer where the list opened. A middle click is its own signal
// (`middleClicked`) and selects nothing.
Rectangle {
    id: entry

    required property int index
    required property string name
    required property bool dir
    required property bool hidden
    required property string mime
    required property real size
    required property real modified

    required property real nameWidth
    // How many of Modified, Type and Size show (the list's shownColumns).
    required property int columns
    required property bool selected
    // A drag over the list would drop into this folder.
    property bool dropTarget: false
    // Rows from where the list opened, while a folder's entries have just
    // come in, or -1: read once, as the row is made.
    required property int riseOrder
    // When the Modified column was worked out, so "Today" moves on.
    required property real now

    signal pressed(int modifiers)
    // Released where it was pressed, without dragging.
    signal clicked(int modifiers)
    signal dragRequested
    signal opened
    signal middleClicked

    height: Tokens.filesRowHeight
    radius: Tokens.menuRowRadius
    color: dropTarget ? Tokens.filesDropTarget : selected ? Tokens.selected : mouse.containsMouse ? Tokens.hover : "transparent"

    component Cell: Text {
        anchors.verticalCenter: parent?.verticalCenter
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSize
        color: Tokens.fgDim
        elide: Text.ElideRight
        textFormat: Text.PlainText
    }

    Row {
        anchors.fill: parent
        anchors.leftMargin: Tokens.rowInset
        anchors.rightMargin: Tokens.rowInset
        spacing: Tokens.rowInset

        Row {
            anchors.verticalCenter: parent.verticalCenter
            width: entry.nameWidth - Tokens.rowInset
            spacing: Tokens.rowInset

            LineIcon {
                id: icon
                anchors.verticalCenter: parent.verticalCenter
                path: Listing.icon(entry)
                color: entry.dir ? Tokens.filesFolder : Tokens.fgDim
                opacity: entry.dir && entry.hidden ? Tokens.filesHiddenIconOpacity : 1
            }
            Cell {
                width: parent.width - icon.width - parent.spacing
                color: entry.hidden ? Tokens.filesFaint : Tokens.fg
                text: Paths.shown(entry.name)
            }
        }
        Cell {
            visible: entry.columns > 0
            width: Tokens.filesColumns[0]
            text: Listing.modified(entry.modified, entry.now)
        }
        Cell {
            visible: entry.columns > 1
            width: Tokens.filesColumns[1]
            text: Listing.kind(entry)
        }
        Cell {
            visible: entry.columns > 2
            width: Tokens.filesColumns[2] - Tokens.rowInset
            horizontalAlignment: Text.AlignRight
            text: Listing.size(entry)
        }
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.MiddleButton
        // A drag takes the rows along; the wheel scrolls the list.
        preventStealing: true

        // Where the press was, until it moved far enough to drag.
        property point pressedAt
        property bool dragging: false

        onPressed: press => {
            pressedAt = Qt.point(press.x, press.y);
            dragging = false;
            if (press.button === Qt.LeftButton)
                entry.pressed(press.modifiers);
        }
        onPositionChanged: move => {
            if ((pressedButtons & Qt.LeftButton) && !dragging && Math.hypot(move.x - pressedAt.x, move.y - pressedAt.y) >= Tokens.filesDragStart) {
                dragging = true;
                entry.dragRequested();
            }
        }
        onClicked: click => {
            if (click.button === Qt.MiddleButton)
                entry.middleClicked();
            else if (!dragging)
                entry.clicked(click.modifiers);
        }
        onDoubleClicked: click => {
            if (click.button === Qt.LeftButton)
                entry.opened();
        }
    }

    transform: Translate {
        id: lift
    }

    Component.onCompleted: {
        if (riseOrder >= 0)
            riseIn.start();
    }

    SequentialAnimation {
        id: riseIn
        PropertyAction {
            target: entry
            property: "opacity"
            value: 0
        }
        PropertyAction {
            target: lift
            property: "y"
            value: Tokens.filesRowRise
        }
        PauseAnimation {
            duration: Math.max(entry.riseOrder, 0) * Tokens.filesRowStagger
        }
        ParallelAnimation {
            NumberAnimation {
                target: entry
                property: "opacity"
                to: 1
                duration: Tokens.durationRowIn
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
            NumberAnimation {
                target: lift
                property: "y"
                to: 0
                duration: Tokens.durationRowIn
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }
    }
}
