pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../daemon"
import "../stage"
import "../slots/slots.js" as Slots
import "../slots"

// One of the bar's rows of pills (`host`: bar.left, bar.center or
// bar.right), filled from the `[[slot]]` entries of the ready features'
// manifests, sorted by their `order`: lowest leftmost. The left row starts
// at the left margin, the right one ends at the right margin, and the
// center one puts its first pill's `middle` in the middle of the screen.
//
// The row lays its pills out by their closed width (BarSlot
// `closedWidth`), so a pill that opens into its panel grows over its
// neighbours instead of pushing them aside, and is drawn over them while
// open. Otherwise the pill nearer the screen's edge is drawn first, the
// same way on both sides.
//
// Each view (ui/slots/Views.qml) is loaded with the bar and the stage, and
// is a BarSlot covering the whole bar.
Item {
    id: row

    // "bar.left", "bar.center" or "bar.right" (Slots.HOST_BAR_*).
    required property string host
    required property Item bar
    required property var stage

    // The slots shown, kept while a features event changes nothing here so
    // the pills (and their open panels) stay.
    property var shown: []
    readonly property var wanted: Slots.forHost(Daemon.features, host)
    onWantedChanged: {
        if (!Slots.same(shown, wanted))
            shown = wanted;
    }
    Component.onCompleted: shown = wanted

    // The views loaded so far, by view id.
    property var loaded: ({})
    // Its views, in order.
    readonly property list<BarSlot> views: shown.map(slot => loaded[slot.view]).filter(view => view)
    // What they hand the stage.
    readonly property list<StagePanel> panels: views.reduce((all, view) => [...all, ...view.panels], [])
    readonly property list<Item> shapes: views.reduce((all, view) => [...all, ...view.shapes], [])
    readonly property bool fading: views.some(view => view.fading)
    readonly property bool raised: views.some(view => view.raised)

    // The view with id `view`, or null.
    function view(id: string): BarSlot {
        return loaded[id] ?? null;
    }

    // Drawn over the other rows while one of its pills is open.
    z: raised ? 1 : 0

    // Where its first pill's `middle` is: the center row puts it in the
    // middle of the screen.
    property real firstMiddle: 0

    // Each pill's place: as wide as the pill closed, gone while it takes no
    // room. The views themselves sit on the row, which never moves.
    Row {
        id: places
        x: row.host === Slots.HOST_BAR_LEFT ? Tokens.barMargin : row.host === Slots.HOST_BAR_RIGHT ? row.width - Tokens.barMargin - width : row.width / 2 - row.firstMiddle
        spacing: Tokens.gap

        Repeater {
            model: row.shown

            Item {
                id: place

                required property var modelData
                required property int index
                readonly property BarSlot view: loader.item as BarSlot

                width: view?.closedWidth ?? 0
                // A Row skips what has no height.
                height: 1
                visible: view?.present ?? false

                Binding {
                    target: place.view
                    property: "slotX"
                    value: places.x + place.x
                    when: place.view !== null
                }
                Binding {
                    target: row
                    property: "firstMiddle"
                    value: place.view?.middle ?? 0
                    when: place.index === 0
                }

                Loader {
                    id: loader
                    parent: row
                    anchors.fill: parent
                    z: place.view?.raised ? 1 : row.host === Slots.HOST_BAR_RIGHT ? -place.index : 0
                    onLoaded: row.loaded = Object.assign({}, row.loaded, {
                        [place.modelData.view]: item
                    })
                    Component.onCompleted: {
                        const url = Views.url(place.modelData.view);
                        if (url.toString() === "") {
                            console.warn(`nova: no view ${place.modelData.view} for ${row.host}`);
                            return;
                        }
                        setSource(url, {
                            bar: row.bar,
                            stage: row.stage
                        });
                    }
                    Component.onDestruction: {
                        const left = Object.assign({}, row.loaded);
                        delete left[place.modelData.view];
                        row.loaded = left;
                    }
                }
            }
        }
    }
}
