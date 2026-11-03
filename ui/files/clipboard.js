.pragma library

// Files on the clipboard as text: one file:// URI per line, as
// text/uri-list spells them, so a terminal or editor pastes them as
// names. Pasting takes only such URIs back: a path copied as plain text
// ("/home") is text, not files to copy.

// The text for the items at `paths` (absolute).
function text(paths) {
    return paths.map(path => "file://" + path.split("/").map(encodeURIComponent).join("/")).join("\n");
}

// The absolute paths `text` names, or none when it names anything else.
function paths(text) {
    const lines = String(text ?? "").split(/\r?\n/).map(line => line.trim()).filter(line => line !== "" && !line.startsWith("#"));
    const found = [];
    for (const line of lines) {
        const path = line.startsWith("file://") ? _local(line) : "";
        if (!path.startsWith("/"))
            return [];
        found.push(path);
    }
    return found;
}

// The path of a file:// URI on this machine, or "" for another host.
function _local(uri) {
    const rest = uri.slice("file://".length);
    const path = rest.startsWith("localhost/") ? rest.slice("localhost".length) : rest;
    try {
        return decodeURIComponent(path);
    } catch (malformed) {
        return "";
    }
}
