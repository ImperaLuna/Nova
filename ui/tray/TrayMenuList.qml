pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "tray.js" as Tray

// A tray app's menu, as Nova draws it: the app's icon and name, the entries
// its DBusMenu lists (checkboxes, separators, submenus with a way back),
// and Nova's own "Hide from tray" (or "Show in tray" for a hidden app).
// Drawn inside the bar's menu panel (TrayMenu) and over quick settings'
// Tray row; TrayMenuOpener reads the entries.
Column {
    id: list

    // The app (a TrayService entry), or null.
    property var entry: null
    property string iconSource: ""

    // The entries, as Quickshell's QsMenuEntry has them, and whether
    // they're a submenu's (TrayMenuOpener).
    property var entries: []
    property bool inSubmenu: false
    // The most it may take: the app's entries scroll past it.
    property real maxHeight: Infinity

    // An entry ran (or Nova's own item): whatever holds the menu closes.
    signal done
    // An entry of the app's menu ran (TrayService.menuPicked).
    signal picked
    signal hideRequested(string key, bool hidden)
    signal submenuRequested(var submenu)
    signal backRequested

    width: Tokens.menuWidth
    spacing: 0
    padding: Tokens.menuPad

    readonly property real rowWidth: width - 2 * padding
    readonly property real separatorHeight: 2 * Tokens.menuSeparatorGap + Tokens.dividerWidth
    // Everything but the app's entries: the padding, the app's row, Back,
    // the separator and Nova's own entry.
    readonly property real chromeHeight: 2 * padding + Tokens.menuRowHeight * (inSubmenu ? 3 : 2) + (entries.length > 0 ? separatorHeight : 0)

    // The app.
    Row {
        width: list.rowWidth
        height: Tokens.menuRowHeight
        spacing: Tokens.iconGap
        leftPadding: Tokens.rowInset

        Image {
            anchors.verticalCenter: parent.verticalCenter
            width: Tokens.trayIconSize
            height: Tokens.trayIconSize
            sourceSize.width: width * 2
            sourceSize.height: height * 2
            source: list.iconSource
            mipmap: true
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width - Tokens.trayIconSize - Tokens.iconGap - Tokens.rowInset
            text: list.entry?.name ?? ""
            // The app's own text.
            textFormat: Text.PlainText
            elide: Text.ElideRight
            color: Tokens.fg
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
            font.weight: Font.DemiBold
        }
    }

    // Back out of a submenu.
    MenuRow {
        visible: list.inSubmenu
        text: qsTr("Back")
        icon: Icons.LEFT
        onClicked: list.backRequested()
    }

    // The app's entries, scrolling when there are more than fit.
    Flickable {
        width: list.rowWidth
        height: Math.min(entryColumn.implicitHeight, Math.max(Tokens.menuRowHeight, list.maxHeight - list.chromeHeight))
        contentHeight: entryColumn.implicitHeight
        boundsBehavior: Flickable.StopAtBounds
        interactive: contentHeight > height
        clip: interactive

        Column {
            id: entryColumn
            width: list.rowWidth

            Repeater {
                model: list.entries

                Item {
                    id: slot
                    required property var modelData
                    width: list.rowWidth
                    height: modelData.isSeparator ? separator.height : row.height

                    Separator {
                        id: separator
                        visible: slot.modelData.isSeparator
                    }

                    MenuRow {
                        id: row
                        visible: !slot.modelData.isSeparator
                        width: parent.width
                        text: Tray.label(slot.modelData.text)
                        enabled: slot.modelData.enabled
                        quits: Tray.isQuit(slot.modelData.text)
                        checkable: slot.modelData.buttonType !== Tray.BUTTON_NONE
                        checked: slot.modelData.checkState === Qt.Checked
                        opens: slot.modelData.hasChildren
                        onClicked: {
                            if (slot.modelData.hasChildren) {
                                list.submenuRequested(slot.modelData);
                                return;
                            }
                            list.picked();
                            slot.modelData.triggered();
                            // A checkbox flips in place; anything else is done.
                            if (!checkable)
                                list.done();
                        }
                    }
                }
            }
        }
    }

    Separator {
        visible: list.entries.length > 0
    }

    MenuRow {
        text: list.entry?.hidden ? qsTr("Show in tray") : qsTr("Hide from tray")
        quiet: true
        onClicked: {
            list.hideRequested(list.entry.key, !list.entry.hidden);
            list.done();
        }
    }

    component Separator: Item {
        width: list.rowWidth
        height: list.separatorHeight

        Rectangle {
            anchors.centerIn: parent
            width: parent.width - 2 * Tokens.menuSeparatorInset
            height: Tokens.dividerWidth
            color: Tokens.outline
        }
    }

    component MenuRow: Rectangle {
        id: menuRow

        property string text: ""
        property string icon: ""
        property bool checkable: false
        property bool checked: false
        // Has a submenu.
        property bool opens: false
        // Ends the app: drawn in the error color.
        property bool quits: false
        // Nova's own entry, set apart.
        property bool quiet: false

        signal clicked

        width: list.rowWidth
        height: Tokens.menuRowHeight
        radius: Tokens.menuRowRadius
        color: press.containsMouse && enabled ? Tokens.hover : "transparent"
        opacity: enabled ? 1 : Tokens.dimOpacity

        readonly property color ink: quits ? Tokens.error : quiet ? Tokens.fgDim : Tokens.fg

        // A checkbox, an icon, or the room either takes, so labels line up.
        Rectangle {
            id: mark
            x: Tokens.rowInset
            anchors.verticalCenter: parent.verticalCenter
            width: Tokens.menuCheckSize
            height: Tokens.menuCheckSize
            radius: Tokens.menuCheckRadius
            color: menuRow.checkable && menuRow.checked ? Tokens.primary : "transparent"
            border.width: menuRow.checkable && !menuRow.checked ? Tokens.strokeThin : 0
            border.color: Tokens.fgDim

            LineIcon {
                anchors.centerIn: parent
                visible: menuRow.checkable && menuRow.checked
                path: Icons.CHECK
                size: Tokens.menuCheckIconSize
                color: Tokens.onAccent
            }
            LineIcon {
                anchors.centerIn: parent
                visible: menuRow.icon !== ""
                path: menuRow.icon || Icons.LEFT
                size: Tokens.iconSizeSmall
                color: menuRow.ink
            }
        }

        Text {
            x: mark.x + mark.width + Tokens.rowInset
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width - x - Tokens.rowInset - (menuRow.opens ? Tokens.iconSizeSmall : 0)
            text: menuRow.text
            // Apps' own labels.
            textFormat: Text.PlainText
            elide: Text.ElideRight
            color: menuRow.ink
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSize
        }

        LineIcon {
            visible: menuRow.opens
            anchors.right: parent.right
            anchors.rightMargin: Tokens.rowInset
            anchors.verticalCenter: parent.verticalCenter
            path: Icons.RIGHT
            size: Tokens.iconSizeSmall
            color: Tokens.fgDim
        }

        MouseArea {
            id: press
            anchors.fill: parent
            hoverEnabled: true
            enabled: menuRow.enabled
            onClicked: menuRow.clicked()
        }
    }
}
