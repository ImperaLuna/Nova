pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// A drive's right-click menu, at the pointer, over the whole window: a
// click beside it closes it. Properties, for a mounted drive, and Eject.
// Eject is only here, never a button on the drive, so it can't be hit by
// accident; it shows for drives that can be unplugged.
Item {
    id: menu

    property var drive: null

    signal ejectChosen(var drive)
    signal propertiesChosen(var drive)

    function open(drive: var, at: point) {
        menu.drive = drive;
        card.x = Math.min(at.x, width - card.width);
        card.y = Math.min(at.y, height - card.height);
    }

    function close() {
        drive = null;
    }

    readonly property bool mounted: drive !== null && drive.mount !== undefined
    visible: drive !== null && (drive.removable || mounted)

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: menu.close()
    }

    component MenuRow: Rectangle {
        id: row

        required property string icon
        required property string label

        signal chosen(var drive)

        width: parent.width
        height: Tokens.menuRowHeight
        radius: Tokens.menuRowRadius
        color: hover.hovered ? Tokens.selected : "transparent"

        Row {
            anchors.left: parent.left
            anchors.leftMargin: Tokens.rowInset
            anchors.verticalCenter: parent.verticalCenter
            spacing: Tokens.rowInset

            LineIcon {
                anchors.verticalCenter: parent.verticalCenter
                path: row.icon
                color: Tokens.filesFaint
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                color: Tokens.fg
                text: row.label
            }
        }

        HoverHandler {
            id: hover
        }
        TapHandler {
            onTapped: {
                const drive = menu.drive;
                menu.close();
                row.chosen(drive);
            }
        }
    }

    Rectangle {
        id: card
        width: Tokens.menuWidth
        height: rows.implicitHeight + 2 * Tokens.menuPad
        radius: Tokens.radiusCard
        color: Tokens.bg
        border.width: Tokens.dividerWidth
        border.color: Tokens.glassRim

        Column {
            id: rows
            x: Tokens.menuPad
            y: Tokens.menuPad
            width: parent.width - 2 * x

            MenuRow {
                visible: menu.mounted
                icon: Icons.DISK
                label: qsTr("Properties")
                onChosen: drive => menu.propertiesChosen(drive)
            }
            MenuRow {
                visible: menu.drive?.removable ?? false
                icon: Icons.EJECT
                label: qsTr("Eject")
                onChosen: drive => menu.ejectChosen(drive)
            }
        }
    }
}
