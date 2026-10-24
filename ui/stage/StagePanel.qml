import QtQuick

// The contract every pill that opens a panel on the stage keeps: subclass
// this as the pill's root and add it to the bar's `panels`. From these alone
// the stage builds its input mask and blur, holds the keyboard and closes
// panels (see StagePanels), so a new panel never wires any of that by hand.
//
// The item's own bounds are its shape: they follow the pill as it grows into
// its panel, and nothing is drawn outside them except a shadow.
Item {
    // Whether its panel is open. The pill sets it on click; the stage clears
    // it on Esc or a click outside.
    property bool open: false
    // Its corner radius as drawn, so clicks at the corners reach the windows
    // below and the blur doesn't show past the rounding.
    property real radius: 0
    // Drawn as glass, so the compositor blurs what's behind it.
    property bool glass: true
    // Whether its open panel takes typing (the launcher will). While it does,
    // the stage holds the keyboard and the whole screen takes clicks, so a
    // click outside the panel closes it.
    property bool wantsKeyboard: false
}
