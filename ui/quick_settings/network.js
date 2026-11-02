.pragma library
.import "../text/bytes.js" as Bytes
.import "../daemon/protocol.js" as Protocol
.import "../theme/icons.js" as Icons

// The network tile's and page's pure logic, from a network event's data
// (novad's `Network`, or null before the first): which link is in use, what
// the tile says, each row's line and role, and how traffic reads. No QML
// types, so ui/tests can check it all.

// The network a connect is going to now, or "": the last attempt while it
// runs, else the network the card is joining on its own.
function connecting(network) {
    var wifi = network && network.wifi;
    if (!wifi)
        return "";
    if (wifi.attempt && wifi.attempt.outcome === "connecting")
        return wifi.attempt.ssid;
    return wifi.state === "connecting" ? (wifi.network || "") : "";
}

// The Ethernet port the Wired row shows: the connected one, else one with a
// cable in, else null (no cable: the row and Prefer are hidden).
function wiredLink(network) {
    var wired = network ? network.wired : [];
    var plugged = null;
    for (var i = 0; i < wired.length; i++) {
        if (wired[i].state === "connected")
            return wired[i];
        if (!plugged && wired[i].state !== "unavailable")
            plugged = wired[i];
    }
    return plugged;
}

function wiredUp(network) {
    var link = wiredLink(network);
    return link !== null && link.state === "connected";
}

// Wi-Fi's switch is on and nothing blocks it: the list shows.
function wifiOn(network) {
    var wifi = network && network.wifi;
    return !!wifi && wifi.enabled && !wifi.blocked;
}

function wifiUp(network) {
    return wifiOn(network) && network.wifi.state === "connected";
}

// Prefer shows while a cable is connected on a machine with Wi-Fi, so the
// pick is there before Wi-Fi connects too.
function preferShown(network) {
    return wiredUp(network) && !!network.wifi;
}

// What a link's row says at its end, as a key for roleLabel: with both
// connected, the one traffic goes over is "use" and the other "backup";
// alone, the Wired row is "use", and Wi-Fi is "connected" (or "use" beside
// a Wired row).
function role(network, kind) {
    if (wiredUp(network) && wifiUp(network))
        return network.primary === kind ? "use" : "backup";
    if (kind === "wired" || wiredLink(network) !== null)
        return "use";
    return "connected";
}

function roleLabel(role) {
    return {
        use: qsTr("In use"),
        backup: qsTr("Backup"),
        connected: qsTr("Connected")
    }[role] || "";
}

// The tile's icon (an Icons path, its dot from iconFill) and the line under
// its title.
function tileIcon(network) {
    if (connecting(network) !== "")
        return Icons.WIFI;
    var primary = network ? network.primary : undefined;
    if (primary === "wired" || (primary === "other" && !wifiUp(network)))
        return Icons.WIRED;
    if (primary)
        return Icons.WIFI;
    return network && network.wifi ? Icons.WIFI_OFF : Icons.WIRED_OFF;
}

// The solid part of one of tileIcon's icons: Wi-Fi's dot.
function iconFill(path) {
    return path === Icons.WIFI || path === Icons.WIFI_OFF ? Icons.WIFI_DOT : "";
}

// The traffic card's title: the link in use, or "Not connected".
function linkName(network) {
    var primary = network ? network.primary : undefined;
    if (primary === "wired")
        return qsTr("Wired");
    if (primary === "wifi")
        return network.wifi.network || qsTr("Wi-Fi");
    return primary === "other" ? qsTr("Connected") : qsTr("Not connected");
}

// The in-use link's speed, or 0 when it has none to give.
function linkMbps(network) {
    var primary = network ? network.primary : undefined;
    if (primary === "wired") {
        var link = wiredLink(network);
        return link && link.speed_mbps ? link.speed_mbps : 0;
    }
    if (primary === "wifi")
        return network.wifi.speed_mbps || 0;
    return 0;
}

// "1 Gb/s", "2.5 Gb/s", "866 Mb/s".
function linkSpeed(mbps) {
    if (mbps >= 1000)
        return parseFloat((mbps / 1000).toFixed(1)) + " Gb/s";
    return mbps + " Mb/s";
}

// The security a network row names: only the kinds that change what a
// click does (no password, or Nova can't join it). The lock on the signal
// shows the rest.
function securityLabel(security) {
    return {
        open: qsTr("Open"),
        wep: "WEP",
        enterprise: qsTr("Enterprise")
    }[security] || "";
}

function bandLabel(band) {
    return {
        "2.4ghz": "2.4 GHz",
        "5ghz": "5 GHz",
        "6ghz": "6 GHz"
    }[band] || "";
}

// Nova connects to it: not WEP, and not 802.1X (set up in the system's
// network settings).
function joinable(net) {
    return net.security !== "wep" && net.security !== "enterprise";
}

// Clicking it asks for a password first: secured and not saved.
function needsPassword(net) {
    return joinable(net) && !net.saved && net.security !== "open";
}

// A network row's second line: "Saved · 2.4 GHz", or how a connect
// to it is going.
function networkLine(network, net) {
    if (connecting(network) === net.ssid)
        return qsTr("Connecting…");
    var attempt = network && network.wifi ? network.wifi.attempt : null;
    if (attempt && attempt.ssid === net.ssid && attempt.outcome === "failed")
        return qsTr("Couldn't connect");
    var parts = [];
    if (net.saved && !net.connected)
        parts.push(qsTr("Saved"));
    parts.push(securityLabel(net.security));
    if (net.band)
        parts.push(bandLabel(net.band));
    return parts.filter(function (part) {
        return part !== "";
    }).join(" · ");
}

// How many of Wi-Fi's three arcs a signal of `strength` (0 to 100) lights.
function arcs(strength) {
    return strength >= 67 ? 3 : strength >= 34 ? 2 : 1;
}

// The network it's on, listed above "Other networks", or null.
function current(network) {
    if (!wifiUp(network))
        return null;
    var networks = network.wifi.networks;
    for (var i = 0; i < networks.length; i++) {
        if (networks[i].connected)
            return networks[i];
    }
    return null;
}

// The rest of the list, in novad's order.
function others(network) {
    if (!wifiOn(network))
        return [];
    var on = current(network);
    return network.wifi.networks.filter(function (net) {
        return net !== on;
    });
}

// A password's length as novad counts it: UTF-8 bytes.
function passwordBytes(text) {
    var bytes = 0;
    for (var i = 0; i < text.length; i++) {
        var code = text.charCodeAt(i);
        if (code < 0x80)
            bytes += 1;
        else if (code < 0x800)
            bytes += 2;
        else if (code >= 0xD800 && code < 0xDC00) {
            // A surrogate pair: one 4-byte character.
            bytes += 4;
            i++;
        } else
            bytes += 3;
    }
    return bytes;
}

// Connect can be pressed: novad takes a password this long.
function passwordFits(text) {
    var bytes = passwordBytes(text);
    return bytes >= Protocol.WPA_PASSWORD_MIN_BYTES && bytes <= Protocol.WIFI_PASSWORD_MAX_BYTES;
}

// Bytes per second as "3.5 MB/s", "114 KB/s".
function speed(bytesPerSecond) {
    return Bytes.bytes(bytesPerSecond) + "/s";
}

// A traffic history as 0-100 for the graph, against `peak`.
function scaled(values, peak) {
    return values.map(function (value) {
        return peak > 0 ? value / peak * 100 : 0;
    });
}

// The fullest second in either history, so both lines share one scale.
function peak(traffic) {
    if (!traffic)
        return 0;
    return Math.max.apply(null, [0].concat(traffic.down, traffic.up));
}
