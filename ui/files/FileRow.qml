import QtQuick
import "../theme"
import "listing.js" as Listing
import "paths.js" as Paths

// A row of Nova Files' Details list: the entry's icon and name, then its
// Modified, Type and Size at the header's column widths; in What uses
// space (`bar` 0..1) a bar for its share of the biggest, then its Size. Hovered rows
// lighten, selected ones fill, a folder a drag would drop into washes in
// the accent; dot files read fainter, and cut ones faded until they're
// pasted. A press that moves far enough asks to drag (`dragRequested`);
// one that doesn't is a click. When a folder's entries have just come in
// (`riseWait`), it eases up into place after the rows nearer where the
// list opened. While `renaming`, its name is a field with the name picked
// up to its extension, as in Explorer: Enter or leaving it renames, Esc
// keeps the name. A middle click is its own signal (`middleClicked`) and
// selects nothing.
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
    // Modified, Type and Size's widths; one 0 wide isn't shown.
    required property var columnWidths
    // Its bar's length in What uses space, or -1 outside it.
    required property real bar
    required property bool selected
    // A drag over the list would drop into this folder.
    property bool dropTarget: false
    property bool cut: false
    property bool renaming: false
    // How long it waits before easing in, in ms, while a folder's entries
    // have just come in, or -1 to just show: read once, as the row is made.
    required property int riseWait
    // When the Modified column was worked out, so "Today" moves on.
    required property real now

    signal pressed(int modifiers)
    // Released where it was pressed, without dragging.
    signal clicked(int modifiers)
    signal dragRequested
    signal opened
    signal middleClicked
    // A right-click at `at` (in the row).
    signal menuAsked(point at)
    // The name field closed: `name` as typed, or "" for Esc.
    signal renameDone(string name)

    height: Tokens.filesRowHeight
    radius: Tokens.menuRowRadius
    color: dropTarget ? Tokens.filesDropTarget : selected ? Tokens.selected : mouse.containsMouse ? Tokens.hover : "transparent"
    opacity: cut ? Tokens.filesCutOpacity : 1

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
                visible: !entry.renaming
                color: entry.hidden ? Tokens.filesFaint : Tokens.fg
                text: Paths.shown(entry.name)
            }
            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                visible: entry.renaming
                width: parent.width - icon.width - parent.spacing
                height: Tokens.filesRowHeight - 2 * Tokens.labelNudge * 2
                radius: Tokens.filesCrumbRadius
                color: Tokens.filesField
                border.width: Tokens.filesFocusRing
                border.color: Tokens.primary

                TextInput {
                    id: field
                    anchors.fill: parent
                    anchors.leftMargin: Tokens.labelNudge * 4
                    anchors.rightMargin: Tokens.labelNudge * 4
                    verticalAlignment: TextInput.AlignVCenter
                    clip: true
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSize
                    color: Tokens.fg
                    selectionColor: Tokens.selected
                    selectedTextColor: Tokens.fg
                    // Answered once: Enter, Esc or leaving it.
                    property bool answered: false

                    function finish(name: string) {
                        if (answered)
                            return;
                        answered = true;
                        entry.renameDone(name);
                    }

                    // After the field's own keys: Enter and Esc answer, and
                    // no other key reaches the list while it's open.
                    Keys.priority: Keys.AfterItem
                    Keys.onPressed: event => {
                        if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
                            finish(text);
                        else if (event.key === Qt.Key_Escape)
                            finish("");
                        event.accepted = true;
                    }
                    onActiveFocusChanged: {
                        if (!activeFocus && entry.renaming)
                            finish(text);
                    }
                }
            }
        }
        Cell {
            width: entry.columnWidths[0]
            visible: entry.bar < 0 && width > 0
            text: Listing.modified(entry.modified, entry.now)
        }
        Cell {
            width: entry.columnWidths[1]
            visible: entry.bar < 0 && width > 0
            text: Listing.kind(entry)
        }
        Rectangle {
            visible: entry.bar >= 0
            anchors.verticalCenter: parent.verticalCenter
            width: entry.columnWidths[0]
            height: Tokens.filesSpaceRowBarHeight
            radius: height / 2
            color: Tokens.track

            Rectangle {
                width: parent.width * Math.max(entry.bar, 0)
                height: parent.height
                radius: parent.radius
                color: Tokens.meter

                Behavior on width {
                    NumberAnimation {
                        duration: Tokens.durationFill
                        easing.type: Easing.BezierSpline
                        easing.bezierCurve: Tokens.easing
                    }
                }
            }
        }
        Cell {
            width: entry.columnWidths[entry.columnWidths.length - 1] - Tokens.rowInset
            visible: width > 0
            horizontalAlignment: Text.AlignRight
            text: Listing.size(entry)
        }
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        enabled: !entry.renaming
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton
        // A drag takes the rows along; the wheel scrolls the list.
        preventStealing: true

        // Where the press was, until it moved far enough to drag.
        property point pressedAt
        property bool dragging: false

        onPressed: press => {
            if (press.button === Qt.RightButton) {
                entry.menuAsked(Qt.point(press.x, press.y));
                return;
            }
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
            else if (click.button === Qt.LeftButton && !dragging)
                entry.clicked(click.modifiers);
        }
        onDoubleClicked: press => {
            if (press.button === Qt.LeftButton)
                entry.opened();
        }
    }
    onRenamingChanged: {
        if (!renaming)
            return;
        field.answered = false;
        // The name as novad spells it, so bytes that aren't UTF-8 stay as
        // they were in a name that's only partly changed.
        field.text = name;
        field.select(0, Listing.stemLength(field.text, dir));
        field.forceActiveFocus();
    }
    Component.onCompleted: {
        if (renaming)
            renamingChanged();
        if (riseWait >= 0)
            riseIn.start();
    }

    transform: Translate {
        id: lift
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
            duration: Math.max(entry.riseWait, 0)
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
