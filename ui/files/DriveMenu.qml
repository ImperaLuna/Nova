import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// A drive's right-click menu, at the pointer, over the whole window: a
// click beside it closes it. Eject is only here, never a button on the
// drive, so it can't be hit by accident; it shows for drives that can be
// unplugged.
Item {
    id: menu

    property var drive: null

    signal ejectChosen(var drive)

    function open(drive: var, at: point) {
        menu.drive = drive;
        card.x = Math.min(at.x, width - card.width);
        card.y = Math.min(at.y, height - card.height);
    }

    function close() {
        drive = null;
    }

    visible: drive !== null && drive.removable

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: menu.close()
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

            Rectangle {
                width: parent.width
                height: Tokens.menuRowHeight
                radius: Tokens.menuRowRadius
                color: ejectHover.hovered ? Tokens.selected : "transparent"

                Row {
                    anchors.left: parent.left
                    anchors.leftMargin: Tokens.rowInset
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: Tokens.rowInset

                    LineIcon {
                        anchors.verticalCenter: parent.verticalCenter
                        path: Icons.EJECT
                        color: Tokens.filesFaint
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        color: Tokens.fg
                        text: qsTr("Eject")
                    }
                }

                HoverHandler {
                    id: ejectHover
                }
                TapHandler {
                    onTapped: {
                        const drive = menu.drive;
                        menu.close();
                        menu.ejectChosen(drive);
                    }
                }
            }
        }
    }
}
