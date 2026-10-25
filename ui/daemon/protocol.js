.pragma library

// Mirror of crates/nova-protocol. One JSON object per line, {"v":2,...}.

var VERSION = 2;

// Commands the UI sends (Command in nova-protocol). A nova-protocol test
// checks every CMD_, EVENT_, TOPIC_, CAPABILITY_, STEP_, POWER_, VOLUME_ and FEATURE_ name
// here (and CAPTURE_, SHOT_) is one novad knows.
var CMD_SUBSCRIBE = "subscribe";
var CMD_SHOW_AI_USAGE = "show_ai_usage";
var CMD_REFRESH_AI_USAGE = "refresh_ai_usage";
var CMD_FOCUS_WORKSPACE = "focus_workspace";
var CMD_STEP_WORKSPACE = "step_workspace";
var CMD_REPORT_LOCKED = "report_locked";
var CMD_REPORT_UNLOCKED = "report_unlocked";
var CMD_POWER_OPTIONS = "power_options";
var CMD_POWER = "power";
var CMD_SAVE_CAPTURE = "save_capture";
var CMD_CANCEL_CAPTURE = "cancel_capture";
var CMD_SCREENSHOT_ACTION = "screenshot_action";
var CMD_START_RECORDING = "start_recording";
var CMD_STOP_RECORDING = "stop_recording";
var CMD_PAUSE_RECORDING = "pause_recording";
var CMD_RECORDING_ACTION = "recording_action";
var CMD_WATCH_RESOURCES = "watch_resources";
var CMD_END_APP = "end_app";
var CMD_SEARCH_APPS = "search_apps";
var CMD_LAUNCH_APP = "launch_app";
var CMD_PIN_APP = "pin_app";
// novad refuses longer search_apps queries (SEARCH_APPS_MAX_CHARS in
// nova-protocol, which checks this matches).
var SEARCH_APPS_MAX_CHARS = 200;

// Events novad sends (Event in nova-protocol). Each topic's events carry the
// topic's name.
var EVENT_HELLO = "hello";
var EVENT_FEATURES = "features";
var EVENT_THEME = "theme";
var EVENT_AI_USAGE = "ai_usage";
var EVENT_COMPOSITOR = "compositor";
var EVENT_TOGGLE = "toggle";
var EVENT_LOCK = "lock";
var EVENT_VOLUME = "volume";
var EVENT_CAPTURE = "capture";
var EVENT_CAPTURE_ENDED = "capture_ended";
var EVENT_SCREENSHOT_SAVED = "screenshot_saved";
var EVENT_PICK_RECORDING = "pick_recording";
var EVENT_RECORDING = "recording";
var EVENT_RECORDING_SAVED = "recording_saved";
var EVENT_HOST = "host";
var EVENT_RESOURCES = "resources";
var EVENT_LAUNCHER = "launcher";

// Event topics a client subscribes to (Topic in nova-protocol).
var TOPIC_FEATURES = "features";
var TOPIC_LOGS = "logs";
var TOPIC_THEME = "theme";
var TOPIC_AI_USAGE = "ai_usage";
var TOPIC_COMPOSITOR = "compositor";
var TOPIC_TOGGLE = "toggle";
var TOPIC_LOCK = "lock";
var TOPIC_VOLUME = "volume";
var TOPIC_SCREENSHOT = "screenshot";
var TOPIC_RECORDING = "recording";
var TOPIC_HOST = "host";
var TOPIC_RESOURCES = "resources";
var TOPIC_LAUNCHER = "launcher";

// What a compositor can report or do (Capability in nova-protocol).
var CAPABILITY_WORKSPACES = "workspaces";
var CAPABILITY_FOCUS_WORKSPACE = "focus_workspace";
var CAPABILITY_FOCUSED_WINDOW = "focused_window";
var CAPABILITY_KEYBOARD_LAYOUT = "keyboard_layout";
var CAPABILITY_LOG_OUT = "log_out";
var CAPABILITY_FULLSCREEN = "fullscreen";

// step_workspace's directions (Step in nova-protocol).
var STEP_NEXT = "next";
var STEP_PREVIOUS = "previous";

// The volume keys a volume event carries (VolumeKey in nova-protocol).
var VOLUME_UP = "up";
var VOLUME_DOWN = "down";
var VOLUME_MUTE = "mute";

// Feature ids (features/<id>/feature.toml).
var FEATURE_AI_USAGE = "ai-usage";
var FEATURE_LOCK = "lock";
var FEATURE_POWER = "power";
var FEATURE_SCREENSHOT = "screenshot";
var FEATURE_RECORDING = "recording";
var FEATURE_QUICK_SETTINGS = "quick-settings";
var FEATURE_LAUNCHER = "launcher";

// Power menu actions (PowerAction in nova-protocol).
var POWER_LOCK = "lock";
var POWER_SUSPEND = "suspend";
var POWER_LOG_OUT = "log_out";
var POWER_REBOOT = "reboot";
var POWER_POWER_OFF = "power_off";

// What the capture toolbar takes (CaptureMode in nova-protocol).
var CAPTURE_REGION = "region";
var CAPTURE_SCREEN = "screen";

// The screenshot card's buttons (ScreenshotAction in nova-protocol).
var SHOT_OPEN = "open";
var SHOT_FOLDER = "folder";
var SHOT_DELETE = "delete";

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

// The workspaces the bar lists for an output (one of a compositor event's
// `outputs`, or null): those with windows, and the one shown there even
// when it's empty.
function barWorkspaces(output) {
    if (!output)
        return [];
    return output.workspaces.filter(function (workspace) {
        return !workspace.empty || workspace.active;
    });
}

// Whether the workspace shown on an output (one of a compositor event's
// `outputs`, or null) has a window filling the whole output, so the bar
// steps aside there.
function fullscreenShown(output) {
    if (!output)
        return false;
    return output.workspaces.some(function (workspace) {
        return workspace.active && workspace.fullscreen;
    });
}
