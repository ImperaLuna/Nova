import QtQuick
import Quickshell.Wayland

// How a surface asks for the keyboard (WlrLayershell.keyboardFocus), from
// its StagePanels. A panel that takes typing holds it outright. A panel that
// closes when you turn to something else (StagePanel `closesOnFocusLoss`)
// offers to take it on demand while the pointer is over its pill, so the
// click that opens it focuses the surface, and while it's open. A click on a
// window then moves the focus there, and still reaches it; losing the focus
// closes the panel. Taking the keyboard outright and then letting it go on
// demand doesn't work: niri hands it straight back to the window.
QtObject {
    id: keyboard

    // The surface's panels.
    required property StagePanels surfacePanels
    // The surface's window (a QQuickWindow).
    required property var window

    readonly property int mode: {
        if (surfacePanels.needsKeyboard)
            return WlrKeyboardFocus.Exclusive;
        if (surfacePanels.offersFocus)
            return WlrKeyboardFocus.OnDemand;
        return WlrKeyboardFocus.None;
    }

    readonly property Connections activity: Connections {
        target: keyboard.window
        function onActiveChanged() {
            // The VM tests read these when a panel doesn't close.
            console.info(`nova: surface ${keyboard.window.active ? "has" : "lost"} the keyboard`);
            if (!keyboard.window.active && keyboard.surfacePanels.followsFocus)
                keyboard.surfacePanels.focusLost();
        }
    }
}
