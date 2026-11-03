import QtQuick
import "../theme"
import "listing.js" as Listing
import "paths.js" as Paths

// A row of Nova Files' Details list: the entry's icon and name, then its
// Modified, Type and Size at the header's column widths. Hovered rows
// lighten, selected ones fill; dot files read fainter. When a folder's
// entries have just come in (`riseOrder`), it eases up into place after
// the rows nearer where the list opened.
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
    // Rows from where the list opened, while a folder's entries have just
    // come in, or -1: read once, as the row is made.
    required property int riseOrder
    // When the Modified column was worked out, so "Today" moves on.
    required property real now

    signal pressed(int modifiers)
    signal opened

    height: Tokens.filesRowHeight
    radius: Tokens.menuRowRadius
    color: selected ? Tokens.selected : mouse.containsMouse ? Tokens.hover : "transparent"

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
        onPressed: press => entry.pressed(press.modifiers)
        onDoubleClicked: entry.opened()
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
