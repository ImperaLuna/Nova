import QtQuick
import Quickshell

// With NOVA_FRAME_LOG=1, logs every frame a stage, PanelSurface or overlay
// window (volume, pop-ups, screenshot card) hands the compositor: which surface it is and the time since the window's
// previous frame. scripts/frames-report.awk turns those lines into missed
// vblanks per surface (`just frames`). Off, it does nothing at all.
Item {
    id: log

    // The stage's is its screen's name, a PanelSurface's
    // "<screen>/<placement>", an overlay's "<screen>/<what>".
    required property string surfaceName

    readonly property bool logging: Quickshell.env("NOVA_FRAME_LOG") === "1"
    property real lastSwap: 0
    // A window built again, or hidden and shown again: the time between
    // isn't a frame's.
    readonly property var frameWindow: Window.window
    onFrameWindowChanged: lastSwap = 0

    Connections {
        target: log.frameWindow
        enabled: log.logging
        function onFrameSwapped() {
            const now = Date.now();
            if (log.lastSwap > 0)
                console.info(`nova-frame: screen=${log.surfaceName} dt=${now - log.lastSwap}`);
            log.lastSwap = now;
        }
        function onVisibleChanged() {
            log.lastSwap = 0;
        }
    }
}
