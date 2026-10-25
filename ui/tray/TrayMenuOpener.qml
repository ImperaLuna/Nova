import QtQuick
import Quickshell

// Reads a tray app's menu for TrayMenuList: the entries of its DBusMenu, or
// of the submenu opened in it, one level at a time. Kept apart from the
// list, so ui/tests can draw the list without Quickshell.
QtObject {
    id: root

    // The app (a TrayService entry), or null.
    property var entry: null
    // Submenus opened, the innermost last.
    property var stack: []
    onEntryChanged: stack = []
    readonly property bool inSubmenu: stack.length > 0

    readonly property QsMenuOpener opener: QsMenuOpener {
        menu: root.stack.length > 0 ? root.stack[root.stack.length - 1] : (root.entry?.item.hasMenu ? root.entry.item.menu : null)
    }
    // `var`, as qmllint can't see the model's values.
    readonly property var model: opener.children
    readonly property var entries: model ? model.values : []

    function open(submenu: var) {
        stack = [...stack, submenu];
    }
    function back() {
        stack = stack.slice(0, -1);
    }
}
