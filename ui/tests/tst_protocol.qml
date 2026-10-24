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
        compare(msg.event, Protocol.EVENT_AI_USAGE);
        compare(msg.data.providers[0].limits[0].percent, 42);
    }

    function test_show_ai_usage_names_the_provider() {
        const line = Protocol.request(5, Protocol.CMD_SHOW_AI_USAGE, {
            provider: "codex"
        });
        compare(line, '{"v":2,"id":5,"cmd":"show_ai_usage","args":{"provider":"codex"}}\n');
    }

    function test_parses_toggle_event() {
        const msg = Protocol.parse('{"v":2,"event":"toggle","data":{"panel":"ai-usage","output":"DP-1"}}');
        compare(msg.event, Protocol.EVENT_TOGGLE);
        compare(msg.data.panel, Protocol.FEATURE_AI_USAGE);
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

    // The compositor event in nova-protocol's wire format snapshot.
    readonly property string compositorLine: '{"v":2,"event":"compositor","data":{"compositor":"niri","capabilities":["workspaces","focused_window"],"outputs":[{"name":"DP-1","focused":true,"workspaces":[{"id":1,"name":"1","active":false,"focused":false,"urgent":false},{"id":2,"name":"web","active":true,"focused":true,"urgent":false}]},{"name":"HDMI-A-1","focused":false,"workspaces":[{"id":3,"name":"1","active":true,"focused":false,"urgent":true}]}],"focused_window":{"app_id":"foot","title":"~"}}}'

    function test_finds_the_focused_workspace() {
        const desktop = Protocol.parse(compositorLine).data;
        compare(Protocol.focusedWorkspace(desktop).name, "web");
        compare(Protocol.focusedWorkspace(null), null);
        desktop.outputs[0].workspaces[1].focused = false;
        compare(Protocol.focusedWorkspace(desktop), null);
    }

    function test_subscribe_can_name_the_compositor_topic() {
        const line = Protocol.request(5, "subscribe", {
            topics: [Protocol.TOPIC_COMPOSITOR]
        });
        compare(line, '{"v":2,"id":5,"cmd":"subscribe","args":{"topics":["compositor"]}}\n');
    }
}
