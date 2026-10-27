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

    function test_parses_volume_event() {
        const msg = Protocol.parse('{"v":2,"event":"volume","data":{"key":"mute"}}');
        compare(msg.event, Protocol.EVENT_VOLUME);
        compare(msg.data.key, Protocol.VOLUME_MUTE);
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
    readonly property string compositorLine: '{"v":2,"event":"compositor","data":{"compositor":"niri","capabilities":["workspaces","focused_window","fullscreen"],"outputs":[{"name":"DP-1","focused":true,"workspaces":[{"id":1,"name":"1","active":false,"focused":false,"urgent":false,"empty":true,"fullscreen":false},{"id":2,"name":"web","active":true,"focused":true,"urgent":false,"empty":false,"fullscreen":false}]},{"name":"HDMI-A-1","focused":false,"workspaces":[{"id":3,"name":"1","active":true,"focused":false,"urgent":true,"empty":false,"fullscreen":true}]}],"focused_window":{"app_id":"foot","title":"~"}}}'

    function test_finds_the_focused_workspace() {
        const desktop = Protocol.parse(compositorLine).data;
        compare(Protocol.focusedWorkspace(desktop).name, "web");
        compare(Protocol.focusedWorkspace(null), null);
        desktop.outputs[0].workspaces[1].focused = false;
        compare(Protocol.focusedWorkspace(desktop), null);
    }

    function test_the_bar_skips_empty_workspaces_but_the_shown_one() {
        const outputs = Protocol.parse(compositorLine).data.outputs;
        compare(Protocol.barWorkspaces(outputs[0]).map(workspace => workspace.name), ["web"]);
        outputs[0].workspaces[1].empty = true;
        compare(Protocol.barWorkspaces(outputs[0]).map(workspace => workspace.name), ["web"], "shown, even when empty");
        outputs[0].workspaces[0].empty = false;
        compare(Protocol.barWorkspaces(outputs[0]).map(workspace => workspace.name), ["1", "web"]);
        compare(Protocol.barWorkspaces(null), []);
    }

    function test_the_bar_keeps_a_special_workspace_while_it_shows_anywhere() {
        const output = Protocol.parse(compositorLine).data.outputs[0];
        output.workspaces.unshift({
            id: -98,
            name: "magic",
            active: false,
            focused: false,
            urgent: false,
            empty: false,
            fullscreen: false,
            special: true
        });
        const names = () => Protocol.barWorkspaces(output).map(workspace => workspace.name);
        compare(names(), ["magic", "web"], "with windows");
        output.workspaces[0].empty = true;
        compare(names(), ["web"], "empty and hidden");
        output.workspaces[0].shown_elsewhere = true;
        compare(names(), ["magic", "web"], "empty, showing on another output");
    }

    function test_the_bar_steps_aside_only_for_a_shown_fullscreen_workspace() {
        const outputs = Protocol.parse(compositorLine).data.outputs;
        compare(Protocol.fullscreenShown(outputs[1]), true);
        compare(Protocol.fullscreenShown(outputs[0]), false);
        outputs[0].workspaces[0].fullscreen = true;
        compare(Protocol.fullscreenShown(outputs[0]), false, "not while that workspace is hidden");
        outputs[0].workspaces[1].fullscreen = true;
        compare(Protocol.fullscreenShown(outputs[0]), true);
        compare(Protocol.fullscreenShown(null), false);
    }

    function test_subscribe_can_name_the_compositor_topic() {
        const line = Protocol.request(5, "subscribe", {
            topics: [Protocol.TOPIC_COMPOSITOR]
        });
        compare(line, '{"v":2,"id":5,"cmd":"subscribe","args":{"topics":["compositor"]}}\n');
    }

    function test_parses_media_event() {
        const msg = Protocol.parse('{"v":2,"event":"media","data":{"players":[{"id":"org.mpris.MediaPlayer2.spotify","identity":"Spotify","status":"playing","title":"Song","artist":"Artist","album":"","browser":false,"length_us":200000000,"position_us":1000000,"position_at_ms":1700000000000,"rate":1.0,"can_next":true,"can_previous":true,"can_play":true,"can_pause":true,"can_seek":true,"can_raise":false}],"shown":"org.mpris.MediaPlayer2.spotify"}}');
        compare(msg.event, Protocol.EVENT_MEDIA);
        compare(msg.data.players[0].status, Protocol.MEDIA_STATUS_PLAYING);
        compare(msg.data.shown, "org.mpris.MediaPlayer2.spotify");
    }

    function test_media_control_names_the_action() {
        const line = Protocol.request(6, Protocol.CMD_MEDIA_CONTROL, {
            player: "org.mpris.MediaPlayer2.spotify",
            action: Protocol.MEDIA_ACTION_PLAY_PAUSE
        });
        compare(line, '{"v":2,"id":6,"cmd":"media_control","args":{"player":"org.mpris.MediaPlayer2.spotify","action":"play_pause"}}\n');
    }

    function test_media_seek_sends_microseconds() {
        const line = Protocol.request(7, Protocol.CMD_MEDIA_SEEK, {
            player: "p",
            position_us: 5000000
        });
        compare(line, '{"v":2,"id":7,"cmd":"media_seek","args":{"player":"p","position_us":5000000}}\n');
    }
}
