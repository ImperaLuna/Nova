import QtQuick
import QtTest
import "../slots/slots.js" as Slots
import "../slots"

// Which views fill a host, from the features event's list (the shape
// app/ sends), and the view registry.
TestCase {
    name: "Slots"

    function feature(id, status, slots) {
        return {
            id: id,
            status: status,
            slots: slots
        };
    }

    function slot(host, view, order) {
        return {
            host: host,
            view: view,
            order: order
        };
    }

    function test_a_host_lists_ready_features_by_order_then_id() {
        const features = [feature("media", "ready", [slot(Slots.HOST_BAR_LEFT, "media.pill", 20), slot(Slots.HOST_STAGE_PANELS, "media.panel", 10)]), feature("ai-usage", "ready", [slot(Slots.HOST_BAR_LEFT, "ai-usage.pill", 10)]), feature("weather", "ready", [slot(Slots.HOST_BAR_LEFT, "weather.pill", 20)]), feature("off", "off", [slot(Slots.HOST_BAR_LEFT, "off.pill", 0)]), feature("lonely", "no_host", [slot(Slots.HOST_BAR_LEFT, "lonely.pill", 0)]),
            {
                id: "core",
                status: "ready"
            }
        ];
        compare(Slots.forHost(features, Slots.HOST_BAR_LEFT).map(found => found.view), ["ai-usage.pill", "media.pill", "weather.pill"]);
        compare(Slots.forHost(features, Slots.HOST_STAGE_PANELS), [
            {
                feature: "media",
                view: "media.panel",
                order: 10
            }
        ]);
        compare(Slots.forHost([], Slots.HOST_BAR_RIGHT), []);
    }

    function test_a_missing_order_counts_as_zero() {
        const features = [feature("b", "ready", [slot(Slots.HOST_BAR_RIGHT, "b.pill", 10)]), feature("a", "ready", [
                {
                    host: Slots.HOST_BAR_RIGHT,
                    view: "a.pill"
                }
            ])];
        compare(Slots.forHost(features, Slots.HOST_BAR_RIGHT).map(found => found.view), ["a.pill", "b.pill"]);
    }

    ListModel {
        id: model
    }

    function shown() {
        const views = [];
        for (let i = 0; i < model.count; i++)
            views.push(model.get(i).view);
        return views;
    }

    function slots(views) {
        return views.map(view => ({
                    feature: view,
                    view: view,
                    order: 0
                }));
    }

    function test_sync_changes_only_what_differs() {
        model.clear();
        Slots.sync(model, slots(["a", "b", "c"]));
        compare(shown(), ["a", "b", "c"]);
        const removed = [];
        const inserted = [];
        const onRemoved = (_, first) => removed.push(first);
        const onInserted = (_, first) => inserted.push(first);
        model.rowsRemoved.connect(onRemoved);
        model.rowsInserted.connect(onInserted);
        Slots.sync(model, slots(["a", "x", "c"]));
        compare(shown(), ["a", "x", "c"]);
        compare(removed, [1], "only b goes");
        compare(inserted, [1], "only x comes");
        Slots.sync(model, slots(["c", "a"]));
        compare(shown(), ["c", "a"]);
        Slots.sync(model, []);
        compare(model.count, 0);
        model.rowsRemoved.disconnect(onRemoved);
        model.rowsInserted.disconnect(onInserted);
    }

    function test_views_map_ids_to_files() {
        verify(Views.url("media.pill").toString().endsWith("/media/NowPlayingSlot.qml"));
        compare(Views.url("nowhere.pill").toString(), "");
    }
}
