.pragma library

// Quick settings' pure logic: the greeting and uptime, which logo to show,
// how sizes and percents read, which apps record from the mic, and the
// resources graph's lines. No QML types, so ui/tests can check it all.

// Distributions with a logo in logos/ (see logos/CREDITS).
var BUNDLED_LOGOS = ["nixos"];

// "Good morning" until noon, "Good afternoon" until 6 pm, then "Good
// evening" until 5 am.
function greeting(hour) {
    if (hour >= 5 && hour < 12)
        return qsTr("Good morning");
    if (hour >= 12 && hour < 18)
        return qsTr("Good afternoon");
    return qsTr("Good evening");
}

// "up 3 h 12 min" for `seconds` of uptime: minutes under an hour, hours and
// minutes under a day, days and hours after that.
function uptime(seconds) {
    var minutes = Math.max(0, Math.floor(seconds / 60));
    var hours = Math.floor(minutes / 60);
    var days = Math.floor(hours / 24);
    if (days > 0)
        return qsTr("up %1 d %2 h").arg(days).arg(hours % 24);
    if (hours > 0)
        return qsTr("up %1 h %2 min").arg(hours).arg(minutes % 60);
    return qsTr("up %1 min").arg(minutes);
}

// Where the header's logo comes from, for a host event's `os`: a bundled
// logo for it or the distribution it derives from (ID_LIKE), else the icon
// theme's LOGO (`iconPath(name)` gives its path, or "" when the theme has
// none), else "" for Nova's own mark.
function logo(os, logosDir, iconPath) {
    if (!os)
        return "";
    var ids = [os.id].concat(os.id_like || []);
    for (var i = 0; i < ids.length; i++) {
        if (BUNDLED_LOGOS.indexOf(ids[i]) >= 0)
            return logosDir + "/" + ids[i] + ".svg";
    }
    return os.logo ? iconPath(os.logo) : "";
}

// A size in bytes, short: "9.4G", "780M", "42K". One decimal under 10.
function shortBytes(bytes) {
    var units = ["B", "K", "M", "G", "T"];
    var value = bytes;
    var unit = 0;
    while (value >= 1000 && unit < units.length - 1) {
        value /= 1024;
        unit++;
    }
    var digits = value < 10 && unit > 0 ? 1 : 0;
    return value.toFixed(digits) + units[unit];
}

// A size in bytes for a list: "2.4 GB", "310 MB".
function bytes(bytes) {
    var short = shortBytes(bytes);
    var unit = short.slice(-1);
    return short.slice(0, -1) + " " + (unit === "B" ? "B" : unit + "B");
}

// A 0-100 share as "23%".
function percent(value) {
    return Math.round(value) + "%";
}

// `used` as a share of `total`, 0 to 1.
function share(used, total) {
    return total > 0 ? Math.min(1, Math.max(0, used / total)) : 0;
}

// The apps recording from the default source right now, by name: the
// targets of its link groups (`groups`, each {source, target}) that are
// app streams, each named once, in link order.
function recorders(source, groups) {
    if (!source)
        return [];
    var names = [];
    for (var i = 0; i < groups.length; i++) {
        var group = groups[i];
        if (!group || !group.source || !group.target || group.source.id !== source.id || !group.target.isStream)
            continue;
        var props = group.target.properties || {};
        var name = props["application.name"] || group.target.description || group.target.name;
        if (name && names.indexOf(name) < 0)
            names.push(name);
    }
    return names;
}

// "Discord is using the mic", "Discord and OBS are using the mic", or ""
// when no app records.
function recordingText(names) {
    if (names.length === 0)
        return "";
    if (names.length === 1)
        return qsTr("%1 is using the mic").arg(names[0]);
    return qsTr("%1 and %2 are using the mic").arg(names.slice(0, -1).join(", ")).arg(names[names.length - 1]);
}

// An SVG path through `values` (each 0 to 100), oldest first, across
// `width` x `height`, its last point at the right edge and spaced as if
// `count` fit the width. `area` closes it along the bottom, for a fill.
function graphPath(values, width, height, count, area) {
    if (values.length === 0)
        return "";
    var step = width / Math.max(1, count - 1);
    var start = width - step * (values.length - 1);
    var path = "";
    for (var i = 0; i < values.length; i++) {
        var x = start + step * i;
        var y = height - Math.min(100, Math.max(0, values[i])) / 100 * height;
        path += (i === 0 ? "M" : "L") + x.toFixed(1) + " " + y.toFixed(1);
    }
    if (area)
        path += "L" + width + " " + height + "L" + start.toFixed(1) + " " + height + "Z";
    return path;
}

// What an output or input is, for a list's second line, from volume.js's
// outputKind.
function kindLabel(kind) {
    return {
        headphones: qsTr("Headphones"),
        bluetooth: qsTr("Bluetooth"),
        display: qsTr("HDMI or DisplayPort"),
        speakers: qsTr("Speakers")
    }[kind] || "";
}

function clamp(x, lo, hi) {
    return Math.max(lo, Math.min(hi, x));
}
