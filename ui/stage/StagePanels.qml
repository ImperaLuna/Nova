import QtQuick

// What the stage needs from everything drawn on it, worked out from the
// bar's StagePanels and its other drawn items: where it takes clicks, where
// it's glass, when it holds the keyboard, and how to close what's open.
QtObject {
    // The pills that open panels.
    property list<StagePanel> panels
    // Drawn items that open nothing, each with a `radius`. They take clicks
    // but aren't glass.
    property list<Item> fixed

    // What takes clicks, each rounded by its `radius`.
    readonly property list<Item> shapes: [...fixed, ...panels]
    // The glass among them, which the compositor blurs behind.
    readonly property list<Item> glassShapes: panels.filter(panel => panel.glass)
    // Whether an open panel takes typing.
    readonly property bool needsKeyboard: panels.some(panel => panel.open && panel.wantsKeyboard)

    // Esc, or a click outside while the keyboard is held: closes the panels
    // holding it. Others, like a notes window, stay open.
    function dismiss() {
        for (const panel of panels) {
            if (panel.wantsKeyboard)
                panel.open = false;
        }
    }
}
