.pragma library

// Mirror of crates/nova-protocol. One JSON object per line, {"v":2,...}.

var VERSION = 2;

// Event topics a client subscribes to (Topic in nova-protocol).
var TOPIC_FEATURES = "features";
var TOPIC_LOGS = "logs";
var TOPIC_THEME = "theme";
var TOPIC_AI_USAGE = "ai_usage";

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
