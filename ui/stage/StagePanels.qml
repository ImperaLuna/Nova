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
    // How far down the screen anything drawn reaches, open panels included,
    // shadows not. The stage is only this tall: the compositor blurs the
    // whole surface behind glass every frame, however small the glass.
    readonly property real reach: Math.max(0, ...shapes.map(item => item.mapToItem(null, 0, item.openHeight ?? item.height).y))
    // Whether an open panel takes typing.
    readonly property bool needsKeyboard: panels.some(panel => panel.open && panel.wantsKeyboard)
    // Whether anything drawn is animating.
    readonly property bool moving: panels.some(panel => panel.moving)

    // `nova toggle <name>` on the screen named `screen`, with `output` the
    // focused one: only that screen toggles, and opening there closes the
    // panel on the others.
    function toggle(name: string, screen: string, output: string) {
        if (output !== screen)
            return;
        for (const panel of panels) {
            if (panel.name === name)
                panel.open = !panel.open;
        }
    }

    // Esc, or a click outside while the keyboard is held: closes the panels
    // holding it. Others, like a notes window, stay open.
    function dismiss() {
        for (const panel of panels) {
            if (panel.wantsKeyboard)
                panel.open = false;
        }
    }
}
