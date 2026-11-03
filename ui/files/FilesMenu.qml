pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// A Nova Files menu (right-click on items, on the empty list or on a
// drive, and the command row's New and Sort), at the pointer, over the
// whole window: a click beside it, Esc or picking an item closes it. Each
// item is {label, icon, shortcut, checked, enabled, act} or {separator:
// true}; one with `items` (a function, read as it opens, so it can follow
// a model that answers late) opens a submenu beside it instead, like Open
// with. The arrows, Enter, Right and Left work it from the keyboard.
FocusScope {
    id: menu

    // What it shows; empty while it's closed.
    property var items: []
    // The submenu's parent row, or -1.
    property int openedAt: -1
    // Where it was asked for, in the menu.
    property point at
    readonly property bool shown: items.length > 0

    signal closed

    // Shows `items` with the menu's corner at `at` (in the window), kept
    // inside it.
    function open(items: var, at: point) {
        openedAt = -1;
        menu.items = items;
        main.current = -1;
        menu.at = at;
        main.forceActiveFocus();
    }

    function close() {
        if (!shown)
            return;
        items = [];
        openedAt = -1;
        closed();
    }

    function pick(item: var) {
        if (!item || item.separator || item.enabled === false || item.items)
            return;
        close();
        item.act?.();
    }

    visible: shown

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.AllButtons
        onPressed: menu.close()
        onWheel: wheel => wheel.accepted = true
    }

    component Card: Rectangle {
        id: card

        property var entries: []
        property int current: -1

        signal hovered(int index)
        // Left and Right: back out of a submenu, or into one.
        signal backRequested
        signal subRequested

        // Row `index`'s top, for the submenu to line up with it.
        function rowTop(index: int): real {
            return (rowRepeater.itemAt(index)?.y ?? 0) + column.y;
        }

        function step(by: int) {
            for (let tries = 0; tries < entries.length; tries++) {
                current = (current + by + entries.length) % entries.length;
                const entry = entries[current];
                if (!entry.separator && entry.enabled !== false)
                    return;
            }
        }

        width: Tokens.filesMenuWidth
        height: column.implicitHeight + 2 * Tokens.menuPad
        radius: Tokens.radiusCard
        color: Tokens.bg
        border.width: Tokens.dividerWidth
        border.color: Tokens.glassRim

        Keys.onPressed: event => {
            if (event.key === Qt.Key_Down)
                step(1);
            else if (event.key === Qt.Key_Up)
                step(-1);
            else if (event.key === Qt.Key_Right)
                subRequested();
            else if (event.key === Qt.Key_Left)
                backRequested();
            else if (event.key === Qt.Key_Escape)
                menu.close();
            else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                if (entries[current]?.items)
                    subRequested();
                else
                    menu.pick(entries[current]);
            } else
                return;
            event.accepted = true;
        }

        // Clicks on the card, a row that can't or a separator, stay on it.
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.AllButtons
        }

        Column {
            id: column
            x: Tokens.menuPad
            y: Tokens.menuPad
            width: parent.width - 2 * x

            Repeater {
                id: rowRepeater
                model: card.entries

                Item {
                    id: row

                    required property var modelData
                    required property int index
                    readonly property bool usable: modelData.enabled !== false

                    width: column.width
                    height: modelData.separator ? 2 * Tokens.filesMenuSeparatorGap + Tokens.dividerWidth : Tokens.filesMenuRowHeight

                    Rectangle {
                        visible: row.modelData.separator === true
                        anchors.verticalCenter: parent.verticalCenter
                        x: Tokens.menuSeparatorInset
                        width: parent.width - 2 * x
                        height: Tokens.dividerWidth
                        color: Tokens.glassRim
                    }
                    Rectangle {
                        visible: !row.modelData.separator
                        anchors.fill: parent
                        radius: Tokens.menuRowRadius
                        opacity: row.usable ? 1 : Tokens.dimOpacity
                        color: card.current === row.index && row.usable ? Tokens.selected : "transparent"

                        LineIcon {
                            id: icon
                            anchors.left: parent.left
                            anchors.leftMargin: Tokens.rowInset
                            anchors.verticalCenter: parent.verticalCenter
                            visible: path !== ""
                            path: row.modelData.checked ? Icons.CHECK : row.modelData.icon ?? ""
                            color: Tokens.fgDim
                        }
                        Text {
                            anchors.left: parent.left
                            anchors.leftMargin: 2 * Tokens.rowInset + Tokens.iconSize
                            anchors.right: hint.left
                            anchors.rightMargin: Tokens.rowInset
                            anchors.verticalCenter: parent.verticalCenter
                            font.family: Tokens.fontSans
                            font.pixelSize: Tokens.fontSize
                            color: Tokens.fg
                            elide: Text.ElideRight
                            textFormat: Text.PlainText
                            text: row.modelData.label ?? ""
                        }
                        Text {
                            id: hint
                            anchors.right: parent.right
                            anchors.rightMargin: Tokens.rowInset
                            anchors.verticalCenter: parent.verticalCenter
                            visible: !row.modelData.items
                            font.family: Tokens.fontMono
                            font.pixelSize: Tokens.fontSizeSmall
                            color: Tokens.filesFaint
                            text: row.modelData.shortcut ?? ""
                        }
                        LineIcon {
                            anchors.right: parent.right
                            anchors.rightMargin: Tokens.rowInset
                            anchors.verticalCenter: parent.verticalCenter
                            visible: row.modelData.items !== undefined
                            size: Tokens.iconSizeMini
                            path: Icons.RIGHT
                            color: Tokens.filesFaint
                        }
                    }

                    HoverHandler {
                        enabled: !row.modelData.separator
                        onHoveredChanged: {
                            if (hovered) {
                                card.current = row.index;
                                card.hovered(row.index);
                            }
                        }
                    }
                    TapHandler {
                        enabled: !row.modelData.separator && row.usable
                        onTapped: {
                            if (row.modelData.items)
                                card.subRequested();
                            else
                                menu.pick(row.modelData);
                        }
                    }
                }
            }
        }
    }

    Card {
        id: main
        entries: menu.items
        focus: true
        // At the pointer, kept inside the window.
        x: Math.max(0, Math.min(menu.at.x, menu.width - width))
        y: Math.max(0, Math.min(menu.at.y, menu.height - height))
        onHovered: index => menu.openedAt = menu.items[index]?.items ? index : -1
        onSubRequested: {
            if (!menu.items[current]?.items)
                return;
            menu.openedAt = current;
            sub.current = -1;
            sub.step(1);
            sub.forceActiveFocus();
        }
        onBackRequested: {}
    }

    Card {
        id: sub
        visible: menu.openedAt >= 0 && entries.length > 0
        entries: menu.openedAt >= 0 ? (menu.items[menu.openedAt]?.items?.() ?? []) : []
        // Beside its row, on the side with room.
        x: main.x + main.width + width <= menu.width ? main.x + main.width - Tokens.menuPad : main.x - width + Tokens.menuPad
        y: Math.max(0, Math.min(main.y + main.rowTop(menu.openedAt) - Tokens.menuPad, menu.height - height))
        onBackRequested: main.forceActiveFocus()
        onSubRequested: {}
    }
}
