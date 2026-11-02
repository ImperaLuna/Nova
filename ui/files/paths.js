.pragma library

// Nova Files' path helpers, kept apart so ui/tests can check them.

// The address bar's parts for `folder` (absolute): one per folder from the
// root down, each {name, path, hidden}, starting at "Home" for a folder in
// `home`. `hidden` marks a dot folder.
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
            name: name,
            path: path,
            hidden: name.startsWith(".")
        });
    }
    return parts;
}

// The last part of `folder`'s path, for the window title; "/" for the root.
function folderName(folder) {
    const name = folder.slice(folder.lastIndexOf("/") + 1);
    return name === "" ? "/" : name;
}

// Whether `path` is `folder` or inside it; nothing is inside "".
function inside(path, folder) {
    return folder !== "" && (path === folder || path.startsWith(folder.endsWith("/") ? folder : folder + "/"));
}
