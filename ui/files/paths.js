.pragma library
.import "../daemon/protocol.js" as Protocol

// Nova Files' path helpers, kept apart so ui/tests can check them: the
// address bar's parts, the window title, and an entry's path. Paths
// and names from novad are wire text: a byte of a name that isn't UTF-8
// comes as a stand-in character (Protocol.RAW_BYTE_BASE + byte), so they
// join and go back to novad as plain strings; only `shown` text is for
// people.

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

// The address bar's parts for `folder` (absolute): one per folder from the
// root down, each {name, path, hidden}, starting at "Home" for a folder in
// `home`. `hidden` marks a dot folder and everything inside one, as the
// sidebar's tree does; `name` is `shown`, `path` is wire text.
function crumbs(folder, home, homeName) {
    const inHome = home !== "" && (folder === home || folder.startsWith(home + "/"));
    const start = inHome ? home : "";
    const parts = [{
            name: inHome ? homeName : "/",
            path: inHome ? home : "/",
            hidden: false
        }];
    let path = start;
    for (const name of folder.slice(start.length).split("/")) {
        if (name === "")
            continue;
        path += "/" + name;
        parts.push({
            name: shown(name),
            path: path,
            hidden: name.startsWith(".") || parts[parts.length - 1].hidden
        });
    }
    return parts;
}

// Where `folder` is, for people: from `~` when it's in `home`, as the
// trash's Original location and the Home page's locations show it.
function located(folder, home) {
    const inHome = home !== "" && (folder === home || folder.startsWith(home + "/"));
    return shown(inHome ? "~" + folder.slice(home.length) : folder);
}

// The last part of `folder`'s path, for the window title; "/" for the root.
function folderName(folder) {
    const name = folder.slice(folder.lastIndexOf("/") + 1);
    return name === "" ? "/" : shown(name);
}

// Whether `path` is `folder` or inside it; nothing is inside "".
function inside(path, folder) {
    return folder !== "" && (path === folder || path.startsWith(folder.endsWith("/") ? folder : folder + "/"));
}

// The folder holding `folder`, or "" for the root.
function parent(folder) {
    if (folder === "/")
        return "";
    const cut = folder.lastIndexOf("/");
    return cut <= 0 ? "/" : folder.slice(0, cut);
}

// The path of the entry `name` in `folder` (absolute), as wire text.
function child(folder, name) {
    return (folder === "/" ? "" : folder) + "/" + name;
}
