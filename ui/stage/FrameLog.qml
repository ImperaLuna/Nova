import QtQuick
import Quickshell

// With NOVA_FRAME_LOG=1, logs every frame a stage or PanelSurface window
// hands the compositor: which surface it is and the time since the window's
// previous frame. scripts/frames-report.awk turns those lines into missed
// vblanks per surface (`just frames`). Off, it does nothing at all.
Item {
    id: log

    // The stage's is its screen's name, a PanelSurface's
    // "<screen>/<placement>".
    required property string surfaceName

    readonly property bool logging: Quickshell.env("NOVA_FRAME_LOG") === "1"
    property real lastSwap: 0

    Connections {
        target: log.Window.window
        enabled: log.logging
        function onFrameSwapped() {
            const now = Date.now();
            if (log.lastSwap > 0)
                console.info(`nova-frame: screen=${log.surfaceName} dt=${now - log.lastSwap}`);
            log.lastSwap = now;
        }
    }
}
