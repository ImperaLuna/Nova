import QtQuick
import "../theme"

// Fades a drawn item out while an open panel on its surface is over it
// (StagePanels `underPanels`), and back in once that's gone:
// `opacity: under ? 0 : 1` with `GiveWay on opacity {}`. Its surface keeps
// drawing while it runs (`running`).
Behavior {
    readonly property bool running: fade.running

    NumberAnimation {
        id: fade
        duration: Tokens.durationFast
    }
}
