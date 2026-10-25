pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol

// Screenshots' link to novad: the region picker's frozen frames while it's
// open, the toolbar's mode, saving or cancelling a pick, and the card's
// buttons. The picker and the card (../) act through here, never through
// Daemon directly.
Singleton {
    id: root

    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_SCREENSHOT)
    // [{output, path}] while the picker is open, else null.
    readonly property var frames: ready ? Daemon.capture : null
    readonly property bool picking: frames !== null
    // The toolbar's Protocol.CAPTURE_* mode; each pick starts on a region.
    property string mode: Protocol.CAPTURE_REGION
    // A pick was sent and novad is saving it: the picker takes no more.
    property bool saving: false
    // The last shot saved ({id, path, width, height, copied}), for the card.
    property var shot: null
    signal saved(var shot)

    onPickingChanged: {
        mode = Protocol.CAPTURE_REGION;
        saving = false;
        // The VM tests wait for these.
        console.info(`nova: region picker ${picking ? "shown" : "hidden"}`);
    }

    // Saves `rect` (frame pixels) of `output`'s frame; the picker closes
    // once novad has it.
    function save(output: string, rect: var) {
        saving = true;
        Daemon.call(Protocol.CMD_SAVE_CAPTURE, {
            output: output,
            rect: rect
        }, error => {
            if (error) {
                console.warn(`nova: saving the screenshot failed: ${error}`);
                root.saving = false;
            }
        });
    }

    function cancel() {
        Daemon.call(Protocol.CMD_CANCEL_CAPTURE);
    }

    // Runs a Protocol.SHOT_* action on a saved shot.
    function act(id: int, action: string) {
        Daemon.call(Protocol.CMD_SCREENSHOT_ACTION, {
            id: id,
            action: action
        });
    }

    Connections {
        target: Daemon
        function onScreenshotSaved(shot: var) {
            root.shot = shot;
            root.saved(shot);
        }
    }
}
