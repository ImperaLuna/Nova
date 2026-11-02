pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "quick_settings.js" as QuickSettings
import "../text/duration.js" as Duration

// The sidebar's first page: the header, then the slots features fill from
// their manifests (features/<id>/feature.toml): toggle tiles two to a row
// (`quick-settings.tiles`), sliders (`.sliders`), cards (`.cards`), one
// view in the room left below them (`.fill`) and one pinned at the foot
// (`.foot`). Each host gets the QML files of its views in slot order and
// loads them with `qs`; a view it can't show now (`present` false) leaves
// no gap. Sections fade in one after another as it opens.
Item {
    id: page

    // The sidebar is open: sections fade in one after another.
    property bool shown: true
    // What every slot gets: the sidebar's state and its requests.
    property QsContext qs: QsContext {}

    // Header.
    property string logo: ""
    property var host: null
    // Room the top row leaves on its right (the pill's buttons).
    property real headerInset: 0

    // Each host's views, as QML file URLs in slot order.
    property var tiles: []
    property var sliders: []
    property var cards: []
    property url fill: ""
    property url foot: ""

    readonly property bool revealing: [header, tileSection, sliderSection, notes, footSection].some(section => section.running) || cardSections.busy

    Column {
        id: sections
        width: page.width
        spacing: Tokens.quickSettingsGap

        Reveal {
            id: header
            width: parent.width
            shown: page.shown
            order: 0

            Header {
                width: parent.width - page.headerInset
                logo: page.logo
                greeting: QuickSettings.greeting(page.qs.now.getHours())
                name: page.host?.user ?? ""
                detail: page.host ? page.host.host + " · " + Duration.uptime(page.qs.now.getTime() / 1000 - page.host.booted_at) : ""
            }
        }

        Reveal {
            id: tileSection
            width: parent.width
            shown: page.shown
            order: 1

            Grid {
                id: grid
                width: parent.width
                columns: 2
                spacing: Tokens.gap
                readonly property real cell: (width - spacing) / columns

                Repeater {
                    model: page.tiles

                    SlotLoader {
                        required property url modelData
                        width: grid.cell
                        view: modelData
                    }
                }
            }
        }

        Reveal {
            id: sliderSection
            width: parent.width
            shown: page.shown
            order: 2

            Column {
                id: sliderColumn
                width: parent.width
                spacing: Tokens.gap

                Repeater {
                    model: page.sliders

                    SlotLoader {
                        required property url modelData
                        width: sliderColumn.width
                        view: modelData
                    }
                }
            }
        }

        // One section each, fading in after the sliders in slot order.
        Repeater {
            id: cardSections
            model: page.cards
            property int revision: 0
            onItemAdded: revision++
            // A card's section is still fading in, or the card settling.
            readonly property bool busy: {
                // itemAt doesn't notify: reading the revision re-evaluates
                // this for new delegates.
                revision;
                let any = false;
                for (let i = 0; i < count; i++) {
                    const section = itemAt(i) as CardSection;
                    any = any || (section?.busy ?? false);
                }
                return any;
            }

            CardSection {}
        }
    }

    Reveal {
        id: notes
        y: sections.height + Tokens.quickSettingsGap
        width: parent.width
        height: footSection.y - y - Tokens.quickSettingsGap
        visible: fillSlot.present
        shown: page.shown
        order: 3 + page.cards.length

        SlotLoader {
            id: fillSlot
            anchors.fill: parent
            view: page.fill
        }
    }

    Reveal {
        id: footSection
        anchors.bottom: parent.bottom
        width: parent.width
        shown: page.shown
        order: 4 + page.cards.length

        SlotLoader {
            width: parent.width
            view: page.foot
        }
    }

    component CardSection: Reveal {
        id: section

        required property url modelData
        required property int index
        readonly property bool busy: running || card.running

        width: sections.width
        shown: page.shown
        order: 3 + index
        visible: card.present

        SlotLoader {
            id: card
            width: parent.width
            view: section.modelData
        }
    }

    // Loads one slot's view with `qs`; nothing when `view` is empty.
    component SlotLoader: Loader {
        id: loader

        property url view
        readonly property QsSlot slot: item as QsSlot
        readonly property bool present: slot?.present ?? false
        readonly property bool running: slot?.running ?? false

        visible: present
        onViewChanged: load()
        Component.onCompleted: load()
        function load() {
            if (view.toString() === "")
                setSource("");
            else
                setSource(view, {
                    qs: page.qs
                });
        }
    }
}
