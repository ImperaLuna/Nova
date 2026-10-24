.pragma library

// Mirror of crates/nova-protocol. One JSON object per line, {"v":2,...}.

var VERSION = 2;

// Commands the UI sends (Command in nova-protocol). A nova-protocol test
// checks every CMD_, EVENT_, TOPIC_, CAPABILITY_, STEP_ and FEATURE_ name
// here is one novad knows.
var CMD_SUBSCRIBE = "subscribe";
var CMD_SHOW_AI_USAGE = "show_ai_usage";
var CMD_REFRESH_AI_USAGE = "refresh_ai_usage";
var CMD_FOCUS_WORKSPACE = "focus_workspace";
var CMD_STEP_WORKSPACE = "step_workspace";

// Events novad sends (Event in nova-protocol). Each topic's events carry the
// topic's name.
var EVENT_HELLO = "hello";
var EVENT_FEATURES = "features";
var EVENT_THEME = "theme";
var EVENT_AI_USAGE = "ai_usage";
var EVENT_COMPOSITOR = "compositor";
var EVENT_TOGGLE = "toggle";

// Event topics a client subscribes to (Topic in nova-protocol).
var TOPIC_FEATURES = "features";
var TOPIC_LOGS = "logs";
var TOPIC_THEME = "theme";
var TOPIC_AI_USAGE = "ai_usage";
var TOPIC_COMPOSITOR = "compositor";
var TOPIC_TOGGLE = "toggle";

// What a compositor can report or do (Capability in nova-protocol).
var CAPABILITY_WORKSPACES = "workspaces";
var CAPABILITY_FOCUS_WORKSPACE = "focus_workspace";
var CAPABILITY_FOCUSED_WINDOW = "focused_window";
var CAPABILITY_KEYBOARD_LAYOUT = "keyboard_layout";

// step_workspace's directions (Step in nova-protocol).
var STEP_NEXT = "next";
var STEP_PREVIOUS = "previous";

// Feature ids (features/<id>/feature.toml).
var FEATURE_AI_USAGE = "ai-usage";

// Returns {kind: "event"|"response", ...} or null for lines to ignore.
function parse(line) {
    var msg;
    try {
        msg = JSON.parse(line);
    } catch (e) {
        return null;
    }
    if (msg === null || typeof msg !== "object" || msg.v !== VERSION)
        return null;
    if (typeof msg.event === "string")
        return { kind: "event", event: msg.event, data: msg.data };
    if (typeof msg.id === "number")
        return { kind: "response", id: msg.id, result: msg.result, error: msg.error };
    return null;
}

function request(id, cmd, args) {
    var msg = { v: VERSION, id: id, cmd: cmd };
    if (args !== undefined)
        msg.args = args;
    return JSON.stringify(msg) + "\n";
}

// Feature ids the UI may load: only those novad reports as ready.
function readyFeatures(features) {
    var ready = [];
    for (var i = 0; i < features.length; i++) {
        if (features[i].status === "ready")
            ready.push(features[i].id);
    }
    return ready;
}

// The workspace shown on the focused output, from a compositor event's
// data, or null.
function focusedWorkspace(desktop) {
    if (!desktop)
        return null;
    for (var i = 0; i < desktop.outputs.length; i++) {
        var workspaces = desktop.outputs[i].workspaces;
        for (var j = 0; j < workspaces.length; j++) {
            if (workspaces[j].focused)
                return workspaces[j];
        }
    }
    return null;
}
