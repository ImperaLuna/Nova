.pragma library
.import "../daemon/protocol.js" as Protocol
.import "trash.js" as Trash

// Nova Files' drag and drop, kept apart so ui/tests can check it: what's
// dragged travels as text/uri-list (file:// URIs), so other apps take it
// and give it. What a drop on a folder does is novad's (files_drop); onto
// the trash it goes to the trash.

// What a drop does besides Protocol.TRANSFER_COPY and TRANSFER_MOVE.
var TRASH = "trash";
var NONE = "";

// The one format drags carry.
var URI_LIST = "text/uri-list";

// The first and last stand-ins for raw bytes (paths.js says why).
var RAW_FIRST = Protocol.RAW_BYTE_BASE + 0x80;
var RAW_LAST = Protocol.RAW_BYTE_BASE + 0xFF;

// Characters a URI path keeps as they are (RFC 3986 unreserved, and "/").
var KEPT = /[A-Za-z0-9\-._~\/]/;

function hex(byte) {
    return "%" + (byte < 16 ? "0" : "") + byte.toString(16).toUpperCase();
}

// The file:// URI of `path` (absolute, wire text): a raw byte goes as
// itself, anything else as its UTF-8 bytes.
function uri(path) {
    let out = "file://";
    for (const character of path) {
        const code = character.codePointAt(0);
        if (code >= RAW_FIRST && code <= RAW_LAST)
            out += hex(code - Protocol.RAW_BYTE_BASE);
        else
            out += KEPT.test(character) ? character : encodeURIComponent(character);
    }
    return out;
}

// The text/uri-list of `paths`: one URI a line, each line ended as the
// format says.
function uriList(paths) {
    return paths.map(path => uri(path) + "\r\n").join("");
}

// The bytes of `text`, where "%XX" is the byte XX and anything else is
// its UTF-8.
function bytesOf(text) {
    const bytes = [];
    for (let at = 0; at < text.length;) {
        if (text[at] === "%" && /^[0-9A-Fa-f]{2}$/.test(text.substr(at + 1, 2))) {
            bytes.push(parseInt(text.substr(at + 1, 2), 16));
            at += 3;
            continue;
        }
        const character = String.fromCodePoint(text.codePointAt(at));
        for (const part of encodeURIComponent(character).match(/%[0-9A-F]{2}|./g))
            bytes.push(part.length === 3 ? parseInt(part.slice(1), 16) : part.charCodeAt(0));
        at += character.length;
    }
    return bytes;
}

// How many bytes the UTF-8 character starting with `lead` takes, or 0
// when no character starts with it.
function sequenceLength(lead) {
    if (lead < 0x80)
        return 1;
    if (lead >= 0xC2 && lead <= 0xDF)
        return 2;
    if (lead >= 0xE0 && lead <= 0xEF)
        return 3;
    if (lead >= 0xF0 && lead <= 0xF4)
        return 4;
    return 0;
}

// `bytes` as wire text: UTF-8 as itself, any other byte as its stand-in.
function wireText(bytes) {
    let out = "";
    for (let at = 0; at < bytes.length;) {
        const length = sequenceLength(bytes[at]);
        const sequence = bytes.slice(at, at + length);
        let decoded = null;
        if (length > 0 && sequence.length === length) {
            try {
                decoded = decodeURIComponent(sequence.map(hex).join(""));
            } catch (notUtf8) {
                decoded = null;
            }
        }
        if (decoded === null) {
            out += String.fromCodePoint(bytes[at] < 0x80 ? bytes[at] : Protocol.RAW_BYTE_BASE + bytes[at]);
            at += 1;
        } else {
            out += decoded;
            at += length;
        }
    }
    return out;
}

// The paths (wire text) of `urls`, a drag's URLs, or [] when any of them
// isn't a file on this machine: a link dragged from a browser drops nowhere.
function localPaths(urls) {
    const paths = [];
    for (const url of urls) {
        const match = /^file:\/\/(localhost)?(\/[^?#]*)$/.exec(String(url));
        if (match === null)
            return [];
        paths.push(wireText(bytesOf(match[2])));
    }
    return paths;
}

// The drag action the pointer shows for `action`.
function dragAction(action) {
    return action === Protocol.TRANSFER_COPY ? Qt.CopyAction : action === NONE ? Qt.IgnoreAction : Qt.MoveAction;
}

// Where nothing can be dropped, for DropTarget.folderAt.
var NOWHERE = null;

// A place that takes drops, for DropTarget.folderAt: `folder` (absolute,
// or the trash's place), opened by hovering when `spring`.
function place(folder, spring) {
    return {
        folder: folder,
        trash: Trash.isTrash(folder),
        spring: spring
    };
}
