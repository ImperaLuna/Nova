import QtQuick
import QtTest
import "../daemon/protocol.js" as Protocol

// Same message shapes as the tests in crates/nova-protocol.
TestCase {
    name: "Protocol"

    function test_parses_hello_event() {
        const msg = Protocol.parse('{"v":1,"event":"hello","data":{"daemon":"0.1.0"}}');
        compare(msg.kind, "event");
        compare(msg.event, "hello");
        compare(msg.data.daemon, "0.1.0");
    }

    function test_parses_responses() {
        compare(Protocol.parse('{"v":1,"id":7,"result":"pong"}').result, "pong");
        compare(Protocol.parse('{"v":1,"id":8,"error":"nope"}').error, "nope");
    }

    function test_ignores_garbage_and_other_versions() {
        compare(Protocol.parse("not json"), null);
        compare(Protocol.parse('{"v":2,"event":"hello"}'), null);
        compare(Protocol.parse("null"), null);
    }

    function test_request_is_one_line() {
        const line = Protocol.request(3, "ping");
        compare(line, '{"v":1,"id":3,"cmd":"ping"}\n');
    }

    function test_only_ready_features_load() {
        const ready = Protocol.readyFeatures([
            { id: "core", status: "ready" },
            { id: "recording", status: "missing_deps", missing: ["wf-recorder"] },
            { id: "weather", status: "off" }
        ]);
        compare(ready, ["core"]);
    }
}
