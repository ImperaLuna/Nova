import QtQuick
import QtTest
import "../daemon/protocol.js" as Protocol

// Same message shapes as the tests in crates/nova-protocol.
TestCase {
    name: "Protocol"

    function test_parses_hello_event() {
        const msg = Protocol.parse('{"v":2,"event":"hello","data":{"daemon":"0.1.0"}}');
        compare(msg.kind, "event");
        compare(msg.event, "hello");
        compare(msg.data.daemon, "0.1.0");
    }

    function test_parses_responses() {
        compare(Protocol.parse('{"v":2,"id":7,"result":"pong"}').result, "pong");
        compare(Protocol.parse('{"v":2,"id":8,"error":"nope"}').error, "nope");
    }

    function test_ignores_garbage_and_other_versions() {
        compare(Protocol.parse("not json"), null);
        compare(Protocol.parse('{"v":1,"event":"hello"}'), null);
        compare(Protocol.parse("null"), null);
    }

    function test_request_is_one_line() {
        const line = Protocol.request(3, "ping");
        compare(line, '{"v":2,"id":3,"cmd":"ping"}\n');
    }

    function test_subscribe_names_topics() {
        const line = Protocol.request(4, "subscribe", {
            topics: [Protocol.TOPIC_FEATURES]
        });
        compare(line, '{"v":2,"id":4,"cmd":"subscribe","args":{"topics":["features"]}}\n');
    }

    function test_parses_ai_usage_event() {
        const msg = Protocol.parse('{"v":2,"event":"ai_usage","data":{"shown":"claude","providers":[{"provider":"claude","state":"ready","limits":[{"window":"session","percent":42,"resets_at":"2026-10-23T11:00:00Z"},{"window":"weekly","percent":61}]},{"provider":"codex","state":"unavailable","hint":"No Codex sessions yet."}]}}');
        compare(msg.event, Protocol.TOPIC_AI_USAGE);
        compare(msg.data.providers[0].limits[0].percent, 42);
    }

    function test_show_ai_usage_names_the_provider() {
        const line = Protocol.request(5, "show_ai_usage", {
            provider: "codex"
        });
        compare(line, '{"v":2,"id":5,"cmd":"show_ai_usage","args":{"provider":"codex"}}\n');
    }

    function test_only_ready_features_load() {
        const ready = Protocol.readyFeatures([
            {
                id: "core",
                status: "ready"
            },
            {
                id: "recording",
                status: "missing_deps",
                missing: ["wf-recorder"]
            },
            {
                id: "weather",
                status: "off"
            }
        ]);
        compare(ready, ["core"]);
    }
}
