.pragma library
.import "trash.js" as Trash

// Nova Files' places that aren't folders, kept apart so ui/tests can
// check them: a window there shows a list of its own, its address bar
// has one part, and there's nothing above it to go up to.

// The places, by path, each {name}.
function _places() {
    return {
        [Trash.PLACE]: {
            name: Trash.name()
        }
    };
}

// Whether `path` is a folder rather than one of these places.
function isFolder(path) {
    return !(path in _places());
}

// The place's name, in the address bar and the window title.
function name(path) {
    return _places()[path].name;
}

// The address bar's one part while a window shows the place at `path`.
function crumbs(path) {
    return [{
            name: name(path),
            path: path,
            hidden: false
        }];
}
