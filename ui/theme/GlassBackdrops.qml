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
    function of(window: QtObject): GlassBackdrop {
        return window ? all.find(backdrop => backdrop.window === window) ?? null : null;
    }
}
