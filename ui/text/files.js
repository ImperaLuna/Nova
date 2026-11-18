.pragma library
.import "../daemon/protocol.js" as Protocol

// Files as QML loads and shows them. Paths and names from novad are wire
// text: a byte of a name that isn't UTF-8 comes as a stand-in character
// (Protocol.RAW_BYTE_BASE + byte), so they go back to novad as plain
// strings; only `shown` text is for people.

// The first and last stand-ins for raw bytes (0x80 to 0xFF).
const RAW_FIRST = Protocol.RAW_BYTE_BASE + 0x80;
const RAW_LAST = Protocol.RAW_BYTE_BASE + 0xFF;

// `text` (a name or path from novad) for people: each run of raw bytes
// shows as one replacement character, as other apps show such names.
function shown(text) {
    let out = "";
    let raw = false;
    for (const character of text) {
        const code = character.codePointAt(0);
        const isRaw = code >= RAW_FIRST && code <= RAW_LAST;
        if (!isRaw)
            out += character;
        else if (!raw)
            out += "\uFFFD";
        raw = isRaw;
    }
    return out;
}

// A `file://` URL for an absolute path, each part escaped so a `#`, `?` or
// space in a name doesn't end the path early.

function fileUrl(path) {
    return "file://" + path.split("/").map(encodeURIComponent).join("/");
}
