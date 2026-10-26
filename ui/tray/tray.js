// The tray's logic, kept out of QML so ui/tests can check it without a
// session bus: which app each of Quickshell's tray items is, what the bar
// shows and in which order, and how a menu reads.

.pragma library

// Quickshell's Status.NeedsAttention (Passive 0, Active 1, NeedsAttention 2).
var STATUS_NEEDS_ATTENTION = 2;

// Quickshell's QsMenuButtonType.None: an entry with no checkbox or radio
// button.
var BUTTON_NONE = 0;

// Menu entries that end the app: shown in the error color, as the mockup
// has them.
var QUIT_WORDS = ["quit", "exit"];

// The tray's apps, in the order they registered: for each of Quickshell's
// `items` ({id, title, status, tooltipTitle, icon}), the matching item of
// novad's `tray` event ({items: [{id, title, app}], hidden}), found by id
// and title, or by id alone when a title changed since. Quickshell doesn't
// give an item's bus address, so each of novad's items matches once: two
// apps' icons with the same id and title (Electron's `chrome_status_icon_1`,
// untitled) pair up in the order both lists have them. While novad lists
// any items, an item it doesn't list isn't shown: novad leaves out the
// X11 bridge's copy of an app that has its own item (Steam twice), and
// lists a new item as soon as it has read it. With none listed, every
// item shows: another program may own the watcher (novad waits behind
// it), and the apps register there. Each entry is
// {item, key, name, icon, appIcon, attention, hidden}: `key` is what
// hide_tray_app knows it by, `appIcon` the desktop entry's icon name ("" when
// novad found no app), `icon` the item's own image.
function entries(items, tray) {
    var known = tray ? tray.items : [];
    var hidden = tray && tray.hidden ? tray.hidden : [];
    var result = [];
    var used = [];
    function find(item, sameTitle) {
        for (var j = 0; j < known.length; j++) {
            if (used.indexOf(j) === -1 && known[j].id === item.id && (!sameTitle || known[j].title === item.title)) {
                used.push(j);
                return known[j];
            }
        }
        return null;
    }
    for (var i = 0; i < items.length; i++) {
        var item = items[i];
        var match = find(item, true) || find(item, false);
        if (known.length > 0 && !match)
            continue;
        var app = match && match.app ? match.app : null;
        var key = app ? app.id : item.id;
        result.push({
            item: item,
            key: key,
            name: (app && app.name) || item.title || item.tooltipTitle || item.id,
            icon: item.icon || "",
            appIcon: app && app.icon ? app.icon : "",
            attention: item.status === STATUS_NEEDS_ATTENTION,
            hidden: hidden.indexOf(key) !== -1
        });
    }
    return result;
}

// What the bar's tray shows: the apps not hidden, those asking for
// attention first, otherwise in order.
function shown(all) {
    var visible = all.filter(function (entry) {
        return !entry.hidden;
    });
    return visible.filter(function (entry) {
        return entry.attention;
    }).concat(visible.filter(function (entry) {
        return !entry.attention;
    }));
}

// Whether any of `all` asks for attention.
function anyAttention(all) {
    return all.some(function (entry) {
        return entry.attention && !entry.hidden;
    });
}

// How many icons are out of sight to the right of a strip showing
// `visible` of `total` icons, scrolled `scrolled` icon widths in.
function moreCount(total, visible, scrolled) {
    return Math.max(0, Math.round(total - visible - scrolled));
}

// Whether a menu entry with `text` ends the app.
function isQuit(text) {
    var word = (text || "").trim().split(/\s+/)[0].toLowerCase();
    return QUIT_WORDS.indexOf(word) !== -1;
}

// A menu entry's text as shown: without the `_` mnemonic markers DBusMenu
// labels carry (`__` is a literal underscore). Electron apps written for
// Windows menus send `&&` for a literal `&` (Ferdium's "Disable
// Notifications && Audio").
function label(text) {
    return (text || "").replace(/__/g, "\u0000").replace(/_/g, "").replace(/\u0000/g, "_").replace(/&&/g, "&");
}
