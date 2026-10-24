pragma Singleton

import QtQuick

// Which screens have something moving on their stage. Qt animates on each
// window's own frames (novad sets QSG_USE_SIMPLE_ANIMATION_DRIVER), and
// while any animation runs, every window draws a frame each refresh, changed
// or not. So while something moves on one screen, the stages on the others
// stop drawing; what changes on them meanwhile shows once nothing moves.
QtObject {
    // The names of the screens whose stage has something moving.
    property list<string> screens: []

    function setMoving(screen: string, moving: bool) {
        const others = screens.filter(name => name !== screen);
        screens = moving ? [...others, screen] : others;
    }
}
