import QtQuick
import Quickshell

// A screen as Nova's surfaces lay themselves out on it: its name and size,
// kept once the screen is gone. When an output goes away (a monitor
// sleeping through a suspend, or unplugged), Quickshell deletes its
// ShellScreen before the surfaces built for it, so in between everything
// bound to the screen would evaluate against null (IMP-92). Surfaces read
// these instead, and keep their last layout until they go too.
QtObject {
    id: held

    // Null once the output is gone, or before there is one.
    required property ShellScreen screen
    // Bound only until it's complete: take() replaces the bindings, so
    // they're right from the start and never see the screen go.
    property string name: screen?.name ?? ""
    property int width: screen?.width ?? 0
    property int height: screen?.height ?? 0

    function take() {
        if (!screen)
            return;
        name = screen.name;
        width = screen.width;
        height = screen.height;
    }
    Component.onCompleted: take()
    onScreenChanged: take()

    readonly property Connections geometry: Connections {
        target: held.screen
        function onGeometryChanged() {
            held.take();
        }
    }
}
