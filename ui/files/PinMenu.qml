import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "home.js" as Home
import "service"

// A folder's right-click menu in the sidebar or on the Home page, at the
// pointer, over the whole window: "Pin to Quick access", or "Unpin from
// Quick access" for one that's pinned, as Explorer words them. A click
// beside it closes it.
Item {
    id: menu

    // The folder it's for (absolute); "" while it's closed.
    property string folder: ""
    readonly property bool pinned: Home.pinnedIn(FilesService.places, folder)

    function open(folder: string, at: point) {
        menu.folder = folder;
        card.x = Math.min(at.x, width - card.width);
        card.y = Math.min(at.y, height - card.height);
    }

    function close() {
        folder = "";
    }

    visible: folder !== ""

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
                color: pinHover.hovered ? Tokens.selected : "transparent"

                Row {
                    anchors.left: parent.left
                    anchors.leftMargin: Tokens.rowInset
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: Tokens.rowInset

                    LineIcon {
                        anchors.verticalCenter: parent.verticalCenter
                        path: Icons.PIN
                        color: Tokens.filesFaint
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        color: Tokens.fg
                        text: menu.pinned ? qsTr("Unpin from Quick access") : qsTr("Pin to Quick access")
                    }
                }

                HoverHandler {
                    id: pinHover
                }
                TapHandler {
                    onTapped: {
                        FilesService.pin(menu.folder, !menu.pinned);
                        menu.close();
                    }
                }
            }
        }
    }
}
