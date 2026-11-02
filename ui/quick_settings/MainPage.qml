pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "quick_settings.js" as QuickSettings
import "../slots/slots.js" as Slots
import "../text/duration.js" as Duration

// The sidebar's first page: the header, then the slots features fill from
// their manifests (features/<id>/feature.toml): toggle tiles two to a row
// (`quick-settings.tiles`), sliders (`.sliders`), cards (`.cards`), one
// view in the room left below them (`.fill`) and one pinned at the foot
// (`.foot`). Each host gets its slots in order and loads their views with
// `qs`, keeping the ones that stay when the slots change; a view it can't
// show now (`present` false) leaves no gap. Sections fade in one after another as it opens.
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

    // Each host's slots in slot order ({view, ...}, slots.js forHost), the
    // fill's and foot's view ids, and the QML file of a view (Views.url).
    property var tiles: []
    property var sliders: []
    property var cards: []
    property string fill: ""
    property string foot: ""
    property var viewUrl: view => ""

    // The views each Repeater shows, kept in step with the slots so a view
    // that stays isn't loaded again.
    ListModel {
        id: tileModel
    }
    ListModel {
        id: sliderModel
    }
    ListModel {
        id: cardModel
    }
    onTilesChanged: Slots.sync(tileModel, tiles)
    onSlidersChanged: Slots.sync(sliderModel, sliders)
    onCardsChanged: Slots.sync(cardModel, cards)
    Component.onCompleted: {
        Slots.sync(tileModel, tiles);
        Slots.sync(sliderModel, sliders);
        Slots.sync(cardModel, cards);
    }

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
                    model: tileModel

                    SlotLoader {
                        width: grid.cell
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
                    model: sliderModel

                    SlotLoader {
                        width: sliderColumn.width
                    }
                }
            }
        }

        // One section each, fading in after the sliders in slot order.
        Repeater {
            id: cardSections
            model: cardModel
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
        order: 3 + cardModel.count

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
        order: 4 + cardModel.count

        SlotLoader {
            width: parent.width
            view: page.foot
        }
    }

    component CardSection: Reveal {
        id: section

        required property string view
        required property int index
        readonly property bool busy: running || card.running

        width: sections.width
        shown: page.shown
        order: 3 + index
        visible: card.present

        SlotLoader {
            id: card
            width: parent.width
            view: section.view
        }
    }

    // Loads one slot's view (an id, `viewUrl` finds its file) with `qs`;
    // nothing when there's no such view.
    component SlotLoader: Loader {
        id: loader

        required property string view
        readonly property QsSlot slot: item as QsSlot
        readonly property bool present: slot?.present ?? false
        readonly property bool running: slot?.running ?? false

        visible: present
        onViewChanged: load()
        Component.onCompleted: load()
        function load() {
            const url = page.viewUrl(view);
            if (url.toString() === "")
                setSource("");
            else
                setSource(url, {
                    qs: page.qs
                });
        }
    }
}
