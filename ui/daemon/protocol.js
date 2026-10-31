.pragma library

// Mirror of crates/nova-protocol. One JSON object per line, {"v":2,...}.

var VERSION = 2;

// Commands the UI sends (Command in nova-protocol). A nova-protocol test
// checks every CMD_, EVENT_, TOPIC_, CAPABILITY_, STEP_, POWER_, VOLUME_ and FEATURE_ name
// here (and CAPTURE_, SHOT_, URGENCY_, MEDIA_, PLAYBACK_) is one novad knows.
var CMD_SUBSCRIBE = "subscribe";
var CMD_SHOW_AI_USAGE = "show_ai_usage";
var CMD_REFRESH_AI_USAGE = "refresh_ai_usage";
var CMD_FOCUS_WORKSPACE = "focus_workspace";
var CMD_STEP_WORKSPACE = "step_workspace";
var CMD_REPORT_LOCKED = "report_locked";
var CMD_REPORT_UNLOCKED = "report_unlocked";
var CMD_POWER_OPTIONS = "power_options";
var CMD_POWER = "power";
var CMD_SCREENSHOT = "screenshot";
var CMD_SAVE_CAPTURE = "save_capture";
var CMD_CANCEL_CAPTURE = "cancel_capture";
var CMD_SCREENSHOT_ACTION = "screenshot_action";
var CMD_START_RECORDING = "start_recording";
var CMD_STOP_RECORDING = "stop_recording";
var CMD_PAUSE_RECORDING = "pause_recording";
var CMD_RECORDING_ACTION = "recording_action";
var CMD_OPEN_LOG = "open_log";
var CMD_WATCH_RESOURCES = "watch_resources";
var CMD_END_APP = "end_app";
var CMD_SEARCH_APPS = "search_apps";
var CMD_APP_ICON = "app_icon";
var CMD_LAUNCH_APP = "launch_app";
var CMD_PIN_APP = "pin_app";
var CMD_HIDE_TRAY_APP = "hide_tray_app";
var CMD_ACTIVATE_TRAY_ITEM = "activate_tray_item";
var CMD_TRAY_MENU_PICKED = "tray_menu_picked";
var CMD_OPEN_NOTIFICATION = "open_notification";
var CMD_NOTIFICATION_ACTION = "notification_action";
var CMD_DISMISS_NOTIFICATION = "dismiss_notification";
var CMD_CLEAR_NOTIFICATIONS = "clear_notifications";
var CMD_READ_NOTIFICATIONS = "read_notifications";
var CMD_SET_DO_NOT_DISTURB = "set_do_not_disturb";
var CMD_MUTE_NOTIFICATION_APP = "mute_notification_app";
var CMD_MEDIA_CONTROL = "media_control";
var CMD_MEDIA_SEEK = "media_seek";
var CMD_MEDIA_SHOW = "media_show";
var CMD_MEDIA_FOCUS = "media_focus";
var CMD_MEDIA_WINDOW = "media_window";
var CMD_BLUETOOTH_POWER = "bluetooth_power";
var CMD_WATCH_BLUETOOTH = "watch_bluetooth";
var CMD_BLUETOOTH_CONNECT = "bluetooth_connect";
var CMD_BLUETOOTH_DISCONNECT = "bluetooth_disconnect";
var CMD_BLUETOOTH_FORGET = "bluetooth_forget";
var CMD_BLUETOOTH_ANSWER = "bluetooth_answer";
var CMD_SET_WALLPAPER = "set_wallpaper";
var CMD_WALLPAPERS = "wallpapers";
var CMD_WALLPAPER_FOLDER = "wallpaper_folder";
// novad refuses longer search_apps queries and media player ids
// (SEARCH_APPS_MAX_CHARS and MEDIA_PLAYER_MAX_BYTES in nova-protocol, which
// checks these match).
var SEARCH_APPS_MAX_CHARS = 200;
var MEDIA_PLAYER_MAX_BYTES = 255;

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
var EVENT_TRAY = "tray";
var EVENT_NOTIFICATIONS = "notifications";
var EVENT_NOTIFICATION_POPUP = "notification_popup";
var EVENT_MEDIA = "media";
var EVENT_PERIPHERALS = "peripherals";
var EVENT_BLUETOOTH = "bluetooth";

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
var TOPIC_TRAY = "tray";
var TOPIC_NOTIFICATIONS = "notifications";
var TOPIC_MEDIA = "media";
var TOPIC_PERIPHERALS = "peripherals";
var TOPIC_BLUETOOTH = "bluetooth";

// What a compositor can report or do (Capability in nova-protocol).
var CAPABILITY_WORKSPACES = "workspaces";
var CAPABILITY_FOCUS_WORKSPACE = "focus_workspace";
var CAPABILITY_FOCUSED_WINDOW = "focused_window";
var CAPABILITY_KEYBOARD_LAYOUT = "keyboard_layout";
var CAPABILITY_LOG_OUT = "log_out";
var CAPABILITY_FULLSCREEN = "fullscreen";
var CAPABILITY_SPECIAL_WORKSPACES = "special_workspaces";

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
var FEATURE_TRAY = "tray";
var FEATURE_NOTIFICATIONS = "notifications";
var FEATURE_MEDIA = "media";
var FEATURE_PERIPHERALS = "peripherals";
var FEATURE_BLUETOOTH = "bluetooth";
var FEATURE_WALLPAPER = "wallpaper";

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

// How much a notification matters (Urgency in nova-protocol).
var URGENCY_LOW = "low";
var URGENCY_NORMAL = "normal";
var URGENCY_CRITICAL = "critical";

// The now-playing panel's buttons (MediaAction in nova-protocol).
var MEDIA_PLAY_PAUSE = "play_pause";
var MEDIA_NEXT = "next";
var MEDIA_PREVIOUS = "previous";

// What a media player is doing (PlaybackStatus in nova-protocol).
var PLAYBACK_PLAYING = "playing";
var PLAYBACK_PAUSED = "paused";
var PLAYBACK_STOPPED = "stopped";

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

// Where the compositor's focus is (a focusedWorkspace, or null), as one
// string that changes when focus moves to another workspace or another
// window on it: "" while it isn't known. It goes by the workspace's
// `active_window`, not `focused_window`, which is empty while a Nova panel
// holds the keyboard.
function focusKey(workspace) {
    if (!workspace)
        return "";
    return workspace.id + "/" + (workspace.active_window ?? "");
}

// The workspaces the bar lists for an output (one of a compositor event's
// `outputs`, or null): those with windows, and the one shown there even
// when it's empty; a special workspace also while it shows on another one.
function barWorkspaces(output) {
    if (!output)
        return [];
    return output.workspaces.filter(function (workspace) {
        return !workspace.empty || workspace.active || workspace.shown_elsewhere === true;
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
