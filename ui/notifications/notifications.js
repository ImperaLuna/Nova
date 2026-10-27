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

// Where `notification` goes among the pop-ups `shown` (each {id}, newest
// first): the index of the one showing it already (it came again, as an
// update), or -1 for a pop-up of its own.
function placeIn(shown, notification) {
    for (var i = 0; i < shown.length; i++) {
        if (shown[i].id === notification.id)
            return i;
    }
    return -1;
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

