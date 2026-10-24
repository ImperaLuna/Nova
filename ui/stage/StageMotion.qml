pragma Singleton

import QtQuick

// Which surfaces have something moving on them. Qt animates on each
// window's own frames (novad sets QSG_USE_SIMPLE_ANIMATION_DRIVER), and
// while any animation runs, every window draws a frame each refresh, changed
// or not. So while something moves on one surface, the others stop drawing;
// what changes on them meanwhile shows once nothing moves.
QtObject {
    // The surfaces (each named "<screen>/<placement>") with something moving.
    property list<string> surfaces: []

    function setMoving(surface: string, moving: bool) {
        const others = surfaces.filter(name => name !== surface);
        surfaces = moving ? [...others, surface] : others;
    }
}
