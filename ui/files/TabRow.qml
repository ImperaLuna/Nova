pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "tabs.js" as Tabs

// Nova Files' tab row along the window's top, as in Windows 11's Explorer:
// a tab per place the window holds (its folder's name, or the trash's),
// the shown one rounded on top and raised in the navigation row's color
// so the two read as one, then + for a new tab. Tabs share the row, each
// between Tokens.filesTabMinWidth and filesTabMaxWidth.
//
// A click shows a tab; a middle click or its × (on the shown or hovered
// tab) closes it. A tab follows the pointer while dragged: let go along
// the row it moves to where it was dropped, let go well above or below
// the row (Tabs.tornOut) it comes out as a window of its own.
Item {
    id: row

    // The window's tabs, in order, and the one shown: FilesTab, or any
    // object with its `title` and `trash`.
    required property var tabs
    required property QtObject current
    // The navigation row's color, which the shown tab takes.
    required property color raised

    signal picked(QtObject tab)
    signal closeRequested(QtObject tab)
    signal newRequested
    signal moved(QtObject tab, int to)
    signal tornOut(QtObject tab)

    readonly property real tabWidth: Tabs.width(width - 2 * Tokens.rowInset - plus.width - Tokens.gap, tabs.length, Tokens.filesTabGap, Tokens.filesTabMinWidth, Tokens.filesTabMaxWidth)

    implicitHeight: Tokens.filesTabPadTop + Tokens.filesTabHeight

    Row {
        id: strip
        anchors.left: parent.left
        anchors.bottom: parent.bottom
        anchors.leftMargin: Tokens.rowInset
        spacing: Tokens.filesTabGap

        Repeater {
            model: row.tabs

            Item {
                id: tab

                required property var modelData
                required property int index
                readonly property bool shown: modelData === row.current
                readonly property bool trash: modelData.trash
                // How far it's been dragged from where it was picked up.
                property point dragged: Qt.point(0, 0)

                width: row.tabWidth
                height: Tokens.filesTabHeight
                z: drag.active ? 1 : 0

                transform: Translate {
                    x: drag.active ? tab.dragged.x : 0
                    y: drag.active ? tab.dragged.y : 0
                }

                // Taller than the tab and clipped, so only its top is round.
                Item {
                    anchors.fill: parent
                    clip: true

                    Rectangle {
                        width: parent.width
                        height: parent.height + Tokens.filesTabRadius
                        radius: Tokens.filesTabRadius
                        color: tab.shown ? row.raised : hover.hovered ? Tokens.hover : "transparent"

                        Behavior on color {
                            ColorAnimation {
                                duration: Tokens.durationFast
                            }
                        }
                    }
                }

                LineIcon {
                    id: icon
                    anchors.left: parent.left
                    anchors.leftMargin: Tokens.rowInset
                    anchors.verticalCenter: parent.verticalCenter
                    path: tab.trash ? Icons.TRASH : Icons.FOLDER
                    color: tab.trash ? Tokens.fgDim : Tokens.filesFolder
                }
                Text {
                    anchors.left: icon.right
                    anchors.right: close.visible ? close.left : parent.right
                    anchors.leftMargin: Tokens.iconGap
                    anchors.rightMargin: Tokens.iconGap
                    anchors.verticalCenter: parent.verticalCenter
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSize
                    font.weight: tab.shown ? Tokens.weightLabel : Tokens.weightBody
                    color: tab.shown ? Tokens.fg : Tokens.fgDim
                    elide: Text.ElideRight
                    textFormat: Text.PlainText
                    text: tab.modelData.title
                }
                Rectangle {
                    id: close
                    anchors.right: parent.right
                    anchors.rightMargin: (Tokens.filesTabHeight - Tokens.filesTabCloseSize) / 2
                    anchors.verticalCenter: parent.verticalCenter
                    width: Tokens.filesTabCloseSize
                    height: Tokens.filesTabCloseSize
                    radius: width / 2
                    visible: tab.shown || hover.hovered
                    color: closeHover.hovered ? Tokens.hover : "transparent"

                    LineIcon {
                        anchors.centerIn: parent
                        size: Tokens.iconSizeMini
                        color: Tokens.fgDim
                        path: Icons.CLOSE
                    }
                    HoverHandler {
                        id: closeHover
                    }
                }

                HoverHandler {
                    id: hover
                }
                // One handler for the tab, so a click on × doesn't also
                // show the tab it closes.
                TapHandler {
                    onTapped: point => {
                        const at = close.mapFromItem(tab, point.position);
                        if (close.visible && close.contains(at))
                            row.closeRequested(tab.modelData);
                        else
                            row.picked(tab.modelData);
                    }
                }
                TapHandler {
                    acceptedButtons: Qt.MiddleButton
                    onTapped: row.closeRequested(tab.modelData)
                }
                DragHandler {
                    id: drag
                    target: null
                    onActiveTranslationChanged: {
                        if (active)
                            tab.dragged = Qt.point(activeTranslation.x, activeTranslation.y);
                    }
                    onActiveChanged: {
                        if (active) {
                            row.picked(tab.modelData);
                            return;
                        }
                        if (Tabs.tornOut(tab.dragged.y, Tokens.filesTabTearDistance, row.tabs.length))
                            row.tornOut(tab.modelData);
                        else
                            row.moved(tab.modelData, Tabs.dropped(tab.index, tab.dragged.x, row.tabWidth + Tokens.filesTabGap, row.tabs.length));
                        tab.dragged = Qt.point(0, 0);
                    }
                }
            }
        }
    }

    ToolButton {
        id: plus
        anchors.left: strip.right
        anchors.leftMargin: Tokens.gap
        anchors.verticalCenter: strip.verticalCenter
        icon: Icons.PLUS
        onClicked: row.newRequested()
    }
}
