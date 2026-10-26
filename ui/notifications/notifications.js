.pragma library

// Notifications' logic without QML, so ui/tests can check it: the quick
// settings list's groups, how long ago something came, and the pop-ups'
// queue. Notifications are a notifications event's items (nova-protocol's
// Notification).

// `items` (newest first) grouped by app, the group with the newest first:
// [{app, items}], each group's items newest first.
function groups(items) {
    var found = [];
    for (var i = 0; i < items.length; i++) {
        var group = null;
        for (var j = 0; j < found.length; j++) {
            if (found[j].app === items[i].app)
                group = found[j];
        }
        if (!group) {
            group = { app: items[i].app, items: [] };
            found.push(group);
        }
        group.items.push(items[i]);
    }
    return found;
}

function unread(items) {
    return items.filter(function (item) {
        return item.unread === true;
    }).length;
}

// How long ago `time` was at `now` (both seconds since the epoch), short:
// "now", "5m", "2h", "3d".
function ago(time, now) {
    var seconds = Math.max(0, now - time);
    if (seconds < 60)
        return qsTr("now");
    if (seconds < 3600)
        return qsTr("%1m").arg(Math.floor(seconds / 60));
    if (seconds < 86400)
        return qsTr("%1h").arg(Math.floor(seconds / 3600));
    return qsTr("%1d").arg(Math.floor(seconds / 86400));
}

// Whether a pop-up of `notification` takes in a newer one from the same
// app, as "+N": plain ones do; critical ones and progress stay their own.
function merges(notification) {
    return notification.urgency !== "critical" && notification.progress === undefined;
}

// Where `notification` goes among the pop-ups `shown` (each {key, id, app,
// mergeable}, newest first): {at, merged} for the one it replaces (the same
// notification, or a mergeable one from its app, which counts one more),
// or null for a new pop-up.
function placeIn(shown, notification) {
    for (var i = 0; i < shown.length; i++) {
        if (shown[i].id === notification.id)
            return { at: i, merged: false };
    }
    if (!merges(notification))
        return null;
    for (var j = 0; j < shown.length; j++) {
        if (shown[j].app === notification.app && shown[j].mergeable)
            return { at: j, merged: true };
    }
    return null;
}

// How long a pop-up of `notification` stays, in ms: what the app asked,
// else `fallback`. 0 is until it's closed: critical ones, and progress
// still running.
function timeout(notification, fallback) {
    if (notification.urgency === "critical")
        return 0;
    if (notification.progress !== undefined && notification.progress < 100)
        return 0;
    return notification.timeout_ms ?? fallback;
}

// What clicking `notification` does: "action" when its app takes the
// click, "launch" when the app can only be opened, "none" otherwise.
function clickDoes(notification) {
    if (notification.live && notification.has_default)
        return "action";
    if (notification.app_id)
        return "launch";
    return "none";
}

// What an Image shows for a picture at the absolute `path`.
function fileUrl(path) {
    return "file://" + path.split("/").map(encodeURIComponent).join("/");
}
