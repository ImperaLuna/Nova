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
    // What takes clicks, each rounded by its `radius` and less its `cutout`:
    // what's shown of each item and its parts (StagePanel `parts`; a pill
    // hides while a panel elsewhere covers it, StagePanel `from`).
    readonly property list<Item> shapes: shownWithParts(items)
    // The glass among them, which the compositor blurs behind.
    readonly property list<Item> glassShapes: shownWithParts(items.filter(item => item.glass === true))
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
    // Whether an open panel takes typing or closes on Esc, so the surface
    // holds the keyboard.
    readonly property bool needsKeyboard: panels.some(panel => panel.open && closesOnEsc(panel))
    // Whether an open panel closes on a click outside it, so the whole
    // screen takes clicks.
    readonly property bool catchesClicks: panels.some(panel => panel.open && panel.closesOnClickOutside)
    // Whether anything drawn is animating.
    readonly property bool moving: panels.some(panel => panel.moving)
    // The drawn items an open panel is over, like the now-playing pill while
    // the AI usage panel beside it is open: they hide meanwhile, clicks and
    // blur too. Glass is see-through, and the compositor blurs only what's
    // below the surface, so they'd show through the panel sharp. An open
    // panel is drawn on top (a closing one isn't), and two open ones never
    // hide each other. Whether an item is shown doesn't count: hiding it
    // mustn't bring it back.
    readonly property list<Item> underPanels: items.filter(item => !(item.open ?? false) && panels.some(panel => panel.open && panel !== item && overlaps(panel, item)))

    // Whether `above` and `below`, on the same surface, share any pixels.
    function overlaps(above: Item, below: Item): bool {
        // Their own boxes, read here so the binding re-checks when either
        // moves or grows: mapping a point isn't watched.
        const aboveBox = Qt.rect(above.x, above.y, above.width, above.height);
        const belowBox = Qt.rect(below.x, below.y, below.width, below.height);
        const at = above.mapToItem(below, 0, 0);
        return at.x < belowBox.width && at.x + aboveBox.width > 0 && at.y < belowBox.height && at.y + aboveBox.height > 0;
    }

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

    // `drawn` and their parts, those of them shown.
    function shownWithParts(drawn: list<Item>): list<Item> {
        // QML's JavaScript has no flatMap.
        return drawn.reduce((all, item) => [...all, item, ...(item.parts ?? [])], []).filter(item => item.visible);
    }

    // Whether (x, y) in its window is on something drawn there, so a click
    // there isn't one outside the panels, even where nothing in the panel
    // takes it.
    function covers(x: real, y: real): bool {
        return shapes.some(item => item.contains(item.mapFromItem(null, x, y)) && !inCutout(item.cutout ?? null, x, y));
    }

    // Whether (x, y) in its window is inside the ellipse that fills `cutout`.
    function inCutout(cutout: Item, x: real, y: real): bool {
        if (!cutout || cutout.width <= 0 || cutout.height <= 0)
            return false;
        const at = cutout.mapFromItem(null, x, y);
        const across = 2 * at.x / cutout.width - 1;
        const down = 2 * at.y / cutout.height - 1;
        return across * across + down * down < 1;
    }

    // Whether Esc closes `panel`: one that takes typing does, and so does
    // one that closes on a click outside, which is every panel but one that
    // opts out, like a notes window.
    function closesOnEsc(panel: StagePanel): bool {
        return panel.wantsKeyboard || panel.closesOnClickOutside;
    }

    // Esc, or a click outside: closes the panels that close on it. Others,
    // like a notes window, stay open.
    function dismiss() {
        for (const panel of panels) {
            if (closesOnEsc(panel))
                panel.open = false;
        }
    }
}
