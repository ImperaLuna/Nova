.pragma library

.import "../theme/icons.js" as Icons

// The Devices card's logic, on novad's peripherals event
// ({devices: [{id, name, kind, link?, battery?, charging, source}]}).

// At or under this percent a battery reads as low.
var LOW_PERCENT = 15;
// How full a battery that only reports a step is drawn.
var LEVEL_FILL = {
    critical: 0.05,
    low: 0.25,
    normal: 0.5,
    high: 0.75,
    full: 1
};
// The steps that read as low.
var LOW_LEVELS = ["critical", "low"];

var KIND_ICONS = {
    mouse: Icons.MOUSE,
    touchpad: Icons.MOUSE,
    keyboard: Icons.KEYBOARD,
    gaming_input: Icons.GAMEPAD,
    headset: Icons.HEADPHONES,
    headphones: Icons.HEADPHONES
};

var LINK_ICONS = {
    dongle: Icons.DONGLE,
    bluetooth: Icons.BLUETOOTH,
    wired: Icons.PLUG
};

// How each link reads, here and on the sound page's rows.
var LINK_LABELS = {
    dongle: qsTr("2.4 GHz"),
    bluetooth: qsTr("Bluetooth"),
    wired: qsTr("Wired"),
    usb: qsTr("USB")
};

function icon(kind) {
    return KIND_ICONS[kind] ?? Icons.DEVICE;
}

function linkIcon(link) {
    return LINK_ICONS[link] ?? "";
}

function linkLabel(link) {
    return LINK_LABELS[link] ?? "";
}

// Whether one name's words, any case, run in order inside the other's, as
// novad matches a headset's two links (`SteelSeries Arctis Nova 5X` and
// `Arctis Nova 5X`).
function namesMatch(a, b) {
    const words = name => name.split(/\s+/).filter(word => word !== "").map(word => word.toLowerCase());
    let short = words(a);
    let long = words(b);
    if (short.length > long.length)
        [short, long] = [long, short];
    if (short.length === 0)
        return false;
    for (let start = 0; start + short.length <= long.length; start++) {
        if (short.every((word, i) => long[start + i] === word))
            return true;
    }
    return false;
}

// How a sound output or input is connected, from volume.js link: a USB
// one is a wireless dongle (2.4 GHz) when a headset novad reads over a
// dongle has its name. PipeWire alone can't tell a dongle from a cable.
function soundLink(link, name, devices) {
    if (link !== "usb")
        return link;
    const dongle = (devices ?? []).some(device => device.link === "dongle" && (device.kind === "headset" || device.kind === "headphones") && namesMatch(device.name, name));
    return dongle ? "dongle" : link;
}

// A device as a row of the card's ListModel: flat roles, -1 percent and ""
// level for what the battery doesn't say.
function row(device) {
    const battery = device.battery ?? {};
    return {
        deviceId: device.id,
        name: device.name,
        kind: device.kind,
        link: device.link ?? "",
        percent: battery.percent ?? -1,
        level: battery.level ?? "",
        charging: device.charging === true
    };
}

// How full its battery is drawn, 0 to 1 (0 when it hasn't said).
function fill(percent, level) {
    if (percent >= 0)
        return Math.min(percent, 100) / 100;
    return LEVEL_FILL[level] ?? 0;
}

function low(percent, level, charging) {
    if (charging)
        return false;
    if (percent >= 0)
        return percent <= LOW_PERCENT;
    return LOW_LEVELS.includes(level);
}
