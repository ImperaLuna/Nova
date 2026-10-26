.pragma library

// Files as QML loads them: a `file://` URL for an absolute path, each part
// escaped so a `#`, `?` or space in a name doesn't end the path early.

function fileUrl(path) {
    return "file://" + path.split("/").map(encodeURIComponent).join("/");
}
