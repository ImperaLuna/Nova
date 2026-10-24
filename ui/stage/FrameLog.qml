import QtQuick
import Quickshell

// With NOVA_FRAME_LOG=1, logs every frame a stage window hands the
// compositor: its screen and the time since the window's previous frame.
// scripts/frames-report.awk turns those lines into missed vblanks per screen
// (`just frames`). Off, it does nothing at all.
Item {
    id: log

    required property string screenName

    readonly property bool logging: Quickshell.env("NOVA_FRAME_LOG") === "1"
    property real lastSwap: 0

    Connections {
        target: log.Window.window
        enabled: log.logging
        function onFrameSwapped() {
            const now = Date.now();
            if (log.lastSwap > 0)
                console.info(`nova-frame: screen=${log.screenName} dt=${now - log.lastSwap}`);
            log.lastSwap = now;
        }
    }
}
