import QtQuick
import "../stage"

// The root of every view in one of the bar's rows (bar.left, bar.center,
// bar.right; BarRow). It covers the whole bar and never moves: its pill
// moves itself to `slotX`, because a pill's input and blur regions only
// follow the pill's own moves, never an ancestor's.
//
// A view sets `pill` and, where they differ from the defaults, its closed
// width, whether it takes room at all, and what it hands the stage. Its
// pill and anything else it draws go inside it (the default property),
// where they fade out while a panel of another pill is over the pill
// (Bar `underPanels`) and hide while a panel on another surface is drawn
// over it (Bar `covered`).
Item {
    id: slot

    // The bar (Bar.qml): its screen, what it covers and so on.
    required property Bar bar
    // The screen's stage (Stage.qml): opens and closes panels by name.
    required property var stage
    // Where the row puts the pill's left edge, in bar coordinates.
    property real slotX: 0

    // The item the row lays out.
    property Item pill: null
    // How much room it takes in the row: its width while closed, so that a
    // pill growing into its panel grows over its neighbours instead of
    // pushing them. 0 takes none, like a pill that isn't shown.
    property real closedWidth: pill?.width ?? 0
    // Whether it takes room in the row at all.
    property bool present: true
    // In the bar.center row, the point of it (from its left edge) that sits
    // in the middle of the screen, when it's the row's first.
    property real middle: closedWidth / 2
    // What it hands the stage: panels (StagePanel) and the other shapes it
    // draws and takes clicks on, each rounded by its `radius`.
    property list<StagePanel> panels: []
    property list<Item> shapes: []
    // One of its panels is open, so it's drawn over its neighbours.
    readonly property bool raised: panels.some(panel => panel.open)
    // It's fading in or out under another pill's panel, so the stage draws.
    readonly property bool fading: fade.running

    default property alias content: layer.data

    Item {
        id: layer
        anchors.fill: parent
        opacity: slot.bar.underPanels.includes(slot.pill) ? 0 : 1
        // Faded out or covered, it takes no clicks and needs no blur.
        visible: !slot.bar.covered.includes(slot.pill) && opacity > 0
        GiveWay on opacity {
            id: fade
        }
    }
}
