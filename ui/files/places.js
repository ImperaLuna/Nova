.pragma library
.import "games.js" as Games
.import "trash.js" as Trash

// Nova Files' places that aren't folders, kept apart so ui/tests can
// check them: a window there shows a list of its own, its address bar
// has one part, and there's nothing above it to go up to.

// Whether `path` is a folder rather than one of these places.
function isFolder(path) {
    return !Games.isGames(path) && !Trash.isTrash(path);
}

// The place's name, in the address bar and the window title.
function name(path) {
    return Games.isGames(path) ? Games.name() : Trash.name();
}

// The address bar's one part while a window shows the place at `path`.
function crumbs(path) {
    return [{
            name: name(path),
            path: path,
            hidden: false
        }];
}
