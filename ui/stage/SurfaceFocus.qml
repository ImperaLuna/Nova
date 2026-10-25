import QtQuick
import Quickshell.Wayland

// How a surface asks for the keyboard (WlrLayershell.keyboardFocus), from
// its StagePanels. A panel that takes typing holds it outright. A panel that
// closes when you turn to something else (StagePanel `closesOnFocusLoss`)
// takes it once as it opens, so the surface is focused, then lets it go back
// on demand: clicking a window or the desktop moves the focus there, and that
// click or scroll still reaches what's under the pointer. Losing the focus
// then closes the panel.
QtObject {
    id: keyboard

    // The surface's panels.
    required property StagePanels surfacePanels
    // The surface's window (a QQuickWindow).
    required property var window

    // The surface has had the focus since such a panel opened.
    property bool held: false
    readonly property int mode: {
        if (surfacePanels.needsKeyboard)
            return WlrKeyboardFocus.Exclusive;
        if (surfacePanels.followsFocus)
            return held ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.Exclusive;
        return WlrKeyboardFocus.None;
    }

    readonly property bool following: surfacePanels.followsFocus
    onFollowingChanged: {
        if (!following)
            held = false;
    }

    readonly property Connections activity: Connections {
        target: keyboard.window
        function onActiveChanged() {
            if (!keyboard.following)
                return;
            // The VM tests read these when a panel doesn't close.
            console.info(`nova: surface ${keyboard.window.active ? "has" : "lost"} the keyboard`);
            if (keyboard.window.active) {
                keyboard.held = true;
            } else if (keyboard.held) {
                keyboard.held = false;
                keyboard.surfacePanels.focusLost();
            }
        }
    }
}
