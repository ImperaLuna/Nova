pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol
import "../../theme"

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
    // The mode the next picker opens in (the toolbar switched from video).
    property string _next: ""
    // The picker was opened by the toolbar's switch.
    property bool switched: false
    // The last shot saved ({id, path, width, height, copied}), for the card.
    property var shot: null
    signal saved(var shot)

    onPickingChanged: {
        switched = picking && _next !== "";
        mode = switched ? _next : Protocol.CAPTURE_REGION;
        _next = "";
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

    // Opens the picker in `mode` once what was on the screen has gone (the
    // toolbar switched from video), so novad's frozen frames don't show it.
    function pick(mode: string) {
        _next = mode;
        opening.restart();
    }

    Timer {
        id: opening
        interval: Tokens.pickerGoneDelay
        onTriggered: Daemon.call(Protocol.CMD_SCREENSHOT, {
            mode: Protocol.CAPTURE_REGION
        }, error => {
            if (error) {
                console.warn(`nova: the screenshot picker didn't open: ${error}`);
                root._next = "";
            }
        })
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
