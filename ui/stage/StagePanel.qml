import QtQuick

// The contract every pill that opens a panel on the stage keeps: subclass
// this as the pill's root and add it to the bar's `panels`. From these alone
// the stage builds its input mask and blur, holds the keyboard and closes
// panels (see StagePanels), so a new panel never wires any of that by hand.
//
// The item's own bounds are its shape: they follow the pill as it grows into
// its panel, and nothing is drawn outside them except a shadow and its
// `parts`.
//
// A panel is open on one screen at a time: opening it closes it on the
// others. Opening an `exclusive` panel also closes every other exclusive one.
Item {
    id: panel

    // Its feature's id (features/<id>/feature.toml), which is also what
    // `nova toggle` calls it, so keep it short.
    required property string name
    // Whether its panel is open. The pill sets it on click; the stage clears
    // it on Esc or a click outside (`closesOnClickOutside`), and when another
    // panel rules it out.
    property bool open: false
    // Its corner radius as drawn, so clicks at the corners reach the windows
    // below and the blur doesn't show past the rounding.
    property real radius: 0
    // Drawn as glass, so the compositor blurs what's behind it.
    property bool glass: true
    // Whether it gives way to other exclusive panels (the launcher, AI
    // usage), or stays open beside them (a notes window would).
    property bool exclusive: true
    // Whether its open panel takes typing (the launcher does). While it
    // does, the stage holds the keyboard, and Esc closes it.
    property bool wantsKeyboard: false
    // Whether Esc or a click outside it closes it while it's open, as every
    // panel does unless it opts out. Meanwhile the whole screen takes clicks,
    // so that click doesn't reach the window below, and the stage holds the
    // keyboard, so Esc reaches it even when it takes no typing. A notes
    // window would stay open instead.
    property bool closesOnClickOutside: true
    // Shapes it draws outside its bounds, like the fillets that curve the
    // power tab into the screen edge. While shown, each takes clicks and is
    // blurred like the panel (the panel's `glass`), as the rect of its own
    // bounds rounded by its `radius`, less the ellipse filling its `cutout`
    // (an Item, or null). Place them on the panel's parent, so the regions
    // follow them as the panel moves: a region only hears its own item move.
    property list<Item> parts
    // Its height once open. The stage is only as tall as what's drawn on it
    // can get, so it makes room for this before the panel opens.
    property real openHeight: height

    // Which surface it's drawn on: "top" is the stage across the top of the
    // screen, with the bar; "right" and "center" are PanelSurfaces, there only
    // while one of their panels is open. A panel on the stage is usually its
    // pill as well; one elsewhere is opened by a pill on the stage, or by
    // `nova toggle`.
    property string placement: "top"
    // Off the stage: everywhere it draws at any point of its morph, open,
    // closed and in between, shadows not included, in screen coordinates
    // (its parent covers the screen). Its surface covers this, and keeps the
    // same size while the panel moves.
    property rect area: Qt.rect(x, y, width, openHeight)
    // Off the stage: an item on the stage it grows out of, like the central
    // pill growing into the launcher. Two surfaces can't share one shape, so
    // it opens right on top of that item, which hides once this has drawn
    // over it (`covering`) and shows again once this has closed back into it.
    property Item from: null
    // Whether it's drawn over `from` on its surface, so `from` can hide.
    // PanelSurface sets it.
    property bool covering: false

    // Whether anything in it is animating right now: its morph, content
    // fading in, a spinner. While a panel moves, the stages on other screens
    // stop drawing (StageMotion), so a pill whose animations aren't covered
    // here would freeze while another screen's panel moves.
    property bool moving: false

    // Whether `other` (a StagePanel) opening closes this one.
    function ruledOutBy(other: var): bool {
        return other !== panel && (other.name === name || (other.exclusive && exclusive));
    }

    onOpenChanged: {
        if (open)
            OpenPanels.opened(panel);
    }

    Connections {
        target: OpenPanels
        enabled: panel.open
        function onOpened(other: Item) {
            if (panel.ruledOutBy(other))
                panel.open = false;
        }
    }
}
