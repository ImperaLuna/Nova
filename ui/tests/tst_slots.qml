import QtQuick
import QtTest
import "../slots/slots.js" as Slots
import "../slots"

// Which views fill a host, from the features event's list (the shape
// crates/novad sends), and the view registry.
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

    function test_same_compares_views_in_order() {
        const one = [
            {
                view: "a"
            },
            {
                view: "b"
            }
        ];
        verify(Slots.same(one, [
            {
                view: "a"
            },
            {
                view: "b"
            }
        ]));
        verify(!Slots.same(one, [
            {
                view: "b"
            },
            {
                view: "a"
            }
        ]));
        verify(!Slots.same(one, one.slice(1)));
    }

    function test_views_map_ids_to_files() {
        verify(Views.url("media.pill").toString().endsWith("/ui/media/NowPlayingSlot.qml"));
        compare(Views.url("nowhere.pill").toString(), "");
    }
}
