pragma Singleton

import QtQuick

// Every window's copy of the screen behind its glass (stage/Backdrop.qml),
// so that glass (GlassTint) finds the one in its own window: a texture only
// draws in the window it was made in. Without one, glass keeps a fixed tint.
QtObject {
    id: backdrops

    property list<GlassBackdrop> all: []

    function add(backdrop: GlassBackdrop) {
        if (!all.includes(backdrop))
            all = [...all, backdrop];
    }
    function remove(backdrop: GlassBackdrop) {
        all = all.filter(other => other !== backdrop);
    }
    // Nova's own windows outside the stage that are on screen (CoversGlass).
    property list<QtObject> covering: []
    readonly property bool covered: covering.length > 0
    function cover(owner: QtObject, shown: bool) {
        const others = covering.filter(other => other !== owner);
        covering = shown ? [...others, owner] : others;
    }

    function of(window: QtObject): GlassBackdrop {
        return window ? all.find(backdrop => backdrop.window === window) ?? null : null;
    }
}
