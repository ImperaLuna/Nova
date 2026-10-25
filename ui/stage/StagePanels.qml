import QtQuick

// What a surface needs from everything drawn on it, worked out from its
// StagePanels and its other drawn items: where it takes clicks, where it's
// glass, how big it has to be, when it holds the keyboard, and how to close
// what's open. Over every panel of a screen, whatever surface each is on,
// it says the same for the whole screen.
QtObject {
    // The pills that open panels.
    property list<StagePanel> panels
    // Drawn items that open nothing, each with a `radius`, and `glass: true`
    // on those drawn as glass. They take clicks.
    property list<Item> fixed

    // Everything drawn, hidden or not.
    readonly property list<Item> items: [...fixed, ...panels]
    // What takes clicks, each rounded by its `radius`: what's shown of it (a
    // pill hides while a panel elsewhere covers it, StagePanel `from`).
    readonly property list<Item> shapes: items.filter(item => item.visible)
    // The glass among them, which the compositor blurs behind.
    readonly property list<Item> glassShapes: items.filter(item => item.glass === true && item.visible)
    // How far down the screen anything drawn reaches, open panels included,
    // shadows not. The stage is only this tall: the compositor blurs the
    // whole surface behind glass every frame, however small the glass.
    readonly property real reach: Math.max(0, ...items.map(item => item.mapToItem(null, 0, item.openHeight ?? item.height).y))
    // The panels open or still closing: off the stage, their surface is
    // there only while some are.
    readonly property list<StagePanel> shown: panels.filter(panel => panel.open || panel.moving)
    // Off the stage: what the shown panels' `area`s cover together, in screen
    // coordinates. Empty while none are shown.
    readonly property rect room: {
        if (shown.length === 0)
            return Qt.rect(0, 0, 0, 0);
        const left = Math.min(...shown.map(panel => panel.area.x));
        const top = Math.min(...shown.map(panel => panel.area.y));
        const right = Math.max(...shown.map(panel => panel.area.x + panel.area.width));
        const bottom = Math.max(...shown.map(panel => panel.area.y + panel.area.height));
        return Qt.rect(left, top, right - left, bottom - top);
    }
    // Whether an open panel takes typing.
    readonly property bool needsKeyboard: panels.some(panel => panel.open && panel.wantsKeyboard)
    // Whether an open panel closes when the focus moves elsewhere.
    readonly property bool followsFocus: panels.some(panel => panel.open && panel.closesOnFocusLoss)
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

    // Whether (x, y) in its window is on something drawn there, so a click
    // there isn't one outside the panels, even where nothing in the panel
    // takes it.
    function covers(x: real, y: real): bool {
        return shapes.some(item => item.contains(item.mapFromItem(null, x, y)));
    }

    // The focus moved elsewhere: closes the panels that close on that.
    function focusLost() {
        for (const panel of panels) {
            if (panel.closesOnFocusLoss)
                panel.open = false;
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
