.pragma library

.import "../theme/icons.js" as Icons

// The Bluetooth tile's and page's logic, on novad's bluetooth event
// ({radio, scanning, devices: [{address, name, kind, status, battery?,
// error?}], pairing?: {id, address, ask: {kind, ...}, incoming}}). No QML
// types, so ui/tests can check it all.

// What a pairing prompt shows with no question (while it's hidden).
var NO_REQUEST = {
    id: 0,
    address: "",
    ask: {
        kind: "authorize"
    },
    incoming: false
};

var KIND_ICONS = {
    headphones: Icons.HEADPHONES,
    headset: Icons.HEADPHONES,
    speaker: Icons.SPEAKER,
    keyboard: Icons.KEYBOARD,
    mouse: Icons.MOUSE,
    gamepad: Icons.GAMEPAD,
    tablet: Icons.DEVICE,
    phone: Icons.DEVICE,
    computer: Icons.MONITOR
};

function icon(kind) {
    return KIND_ICONS[kind] ?? Icons.BLUETOOTH;
}

function kindName(kind) {
    switch (kind) {
    case "headphones":
        return qsTr("Headphones");
    case "headset":
        return qsTr("Headset");
    case "speaker":
        return qsTr("Speaker");
    case "keyboard":
        return qsTr("Keyboard");
    case "mouse":
        return qsTr("Mouse");
    case "gamepad":
        return qsTr("Gamepad");
    case "tablet":
        return qsTr("Tablet");
    case "phone":
        return qsTr("Phone");
    case "computer":
        return qsTr("Computer");
    }
    return qsTr("Device");
}

function isOn(state) {
    return state?.radio === "on";
}

// The tile's line: why it's off, else the one connected device's name or
// how many are.
function tileSubtitle(state) {
    switch (state?.radio ?? "missing") {
    case "missing":
        return qsTr("No adapter");
    case "switched_off":
        return qsTr("Off by hardware switch");
    case "off":
        return qsTr("Off");
    }
    var connected = state.devices.filter(function (device) {
        return device.status === "connected";
    });
    if (connected.length === 0)
        return qsTr("On");
    if (connected.length === 1)
        return connected[0].name;
    return qsTr("%1 connected").arg(connected.length);
}

// The page's lists: devices in use or on their way there under Connected,
// known ones under My devices, the rest under Nearby. novad sends them in
// that order already.
function section(device) {
    switch (device.status) {
    case "connected":
    case "connecting":
    case "disconnecting":
        return "connected";
    case "paired":
        return "mine";
    }
    // Nearby, or being paired.
    return "nearby";
}

function inSection(devices, key) {
    return devices.filter(function (device) {
        return section(device) === key;
    });
}

// What a row says on its right while novad works on it, or "" for idle.
function busyText(status, waiting) {
    switch (status) {
    case "pairing":
        return waiting ? qsTr("Waiting for you") : qsTr("Pairing");
    case "connecting":
        return qsTr("Connecting");
    case "disconnecting":
        return qsTr("Disconnecting");
    }
    return "";
}

// The line under a row's name.
function rowSubtitle(device) {
    if (device.status === "connected")
        return qsTr("%1 · connected").arg(kindName(device.kind));
    return kindName(device.kind);
}

// What clicking a row does: "connect" (pairs a new one first), "open"
// (shows Disconnect and Forget) or "" while it's busy.
function click(device) {
    switch (device.status) {
    case "nearby":
    case "paired":
        return "connect";
    case "connected":
        return "open";
    }
    return "";
}

// The question a pairing prompt asks about `name`, by the request's
// `ask.kind`, and whether a phone started it.
function question(ask, name, incoming) {
    switch (ask.kind) {
    case "confirm":
        return incoming ? qsTr("%1 wants to pair. Does it show this code?").arg(name) : qsTr("Does %1 show this code?").arg(name);
    case "enter_pin":
        return qsTr("Enter the PIN for %1").arg(name);
    case "enter_passkey":
        return qsTr("Enter the code %1 shows").arg(name);
    case "show_pin":
    case "show_passkey":
        return qsTr("Type this on %1, then press Enter").arg(name);
    case "authorize":
        return qsTr("%1 wants to connect. Allow it?").arg(name);
    }
    return "";
}

// The code a prompt shows, or "" when it asks for one.
function code(ask) {
    switch (ask.kind) {
    case "confirm":
    case "show_passkey":
        return ask.code;
    case "show_pin":
        return ask.pin;
    }
    return "";
}

// Whether the user types an answer.
function typed(ask) {
    return ask.kind === "enter_pin" || ask.kind === "enter_passkey";
}

// Whether the prompt waits for a yes (a code to type there needs none:
// only Cancel).
function answers(ask) {
    return ask.kind !== "show_pin" && ask.kind !== "show_passkey";
}

// The prompt's buttons: [turn down, yes]; yes is "" for show-only asks.
function buttons(ask, incoming) {
    if (!answers(ask))
        return [qsTr("Cancel"), ""];
    if (ask.kind === "authorize")
        return [qsTr("Decline"), qsTr("Allow")];
    if (incoming)
        return [qsTr("Decline"), qsTr("Pair")];
    if (ask.kind === "confirm")
        return [qsTr("Cancel"), qsTr("It matches")];
    return [qsTr("Cancel"), qsTr("Pair")];
}

// Whether `text` answers a typed ask: a PIN is 1 to 16 printable
// characters, a passkey up to 6 digits (BlueZ's limits, as novad checks).
function validCode(ask, text) {
    if (ask.kind === "enter_pin")
        return /^[\x21-\x7e]{1,16}$/.test(text);
    if (ask.kind === "enter_passkey")
        return /^[0-9]{1,6}$/.test(text.trim());
    return false;
}

// The name a prompt calls the device by: its row's, else its address.
function deviceName(devices, address) {
    var device = devices.find(function (device) {
        return device.address === address;
    });
    return device ? device.name : address;
}
