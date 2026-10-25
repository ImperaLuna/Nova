pragma Singleton

import QtQuick
import Quickshell
import "../../daemon"
import "../../daemon/protocol.js" as Protocol
import "../../theme"

// Recording's link to novad: the picker in video mode (`nova record` opens
// and closes it), the toolbar's mode and sound, the countdown, what's being
// recorded and for how long, and the card's buttons. The picker, the
// recording pill and the card (../, ../../bar) act through here, never
// through Daemon directly.
Singleton {
    id: root

    readonly property bool ready: Daemon.readyFeatures.includes(Protocol.FEATURE_RECORDING)
    // The picker shows on every screen, in video mode.
    property bool picking: false
    // The toolbar's Protocol.CAPTURE_* mode; each pick starts on a region.
    property string mode: Protocol.CAPTURE_REGION
    // The sound to record, kept from one recording to the next.
    property bool desktopAudio: true
    property bool micAudio: false
    // Counting down to the start: what's left, 0 when not counting.
    property int countdown: 0
    // What the countdown starts: {output, region (layout px, or null)}.
    property var _pending: null

    // What's being recorded ({output, region, audio, elapsed_ms, paused,
    // can_pause}), or null.
    readonly property var active: ready ? Daemon.recording : null
    readonly property bool recording: active !== null
    // How long it has recorded, in ms: novad's count when it last said,
    // plus the time since while it isn't paused. `now` ticks while it runs.
    readonly property real elapsed: active ? active.elapsed_ms + (active.paused ? 0 : now - _since) : 0
    property real now: Date.now()
    property real _since: Date.now()

    // The last recording saved ({id, path, duration_ms}), for the card.
    signal saved(var saved)

    onActiveChanged: {
        _since = Date.now();
        now = _since;
    }
    onPickingChanged: {
        mode = Protocol.CAPTURE_REGION;
        // A count that was going is cancelled with the picker.
        step.stop();
        countdown = 0;
        _pending = null;
        // The VM tests wait for these.
        console.info(`nova: recording picker ${picking ? "shown" : "hidden"}`);
    }
    onReadyChanged: {
        if (!ready)
            picking = false;
    }

    // Counts down, then starts recording `output`, or `region` of the
    // layout on it.
    function record(output: string, region: var) {
        if (countdown > 0)
            return;
        _pending = {
            output: output,
            region: region
        };
        countdown = Tokens.recordCountdown;
        step.restart();
    }

    function cancel() {
        picking = false;
    }

    // Opens the picker in `mode` (the toolbar switched from a screenshot).
    function pick(mode: string) {
        if (!ready || recording)
            return;
        picking = true;
        root.mode = mode;
    }

    function stop() {
        Daemon.call(Protocol.CMD_STOP_RECORDING);
    }

    function pause(paused: bool) {
        Daemon.call(Protocol.CMD_PAUSE_RECORDING, {
            paused: paused
        });
    }

    // Runs a Protocol.SHOT_* action on a saved recording.
    function act(id: int, action: string) {
        Daemon.call(Protocol.CMD_RECORDING_ACTION, {
            id: id,
            action: action
        });
    }

    Timer {
        id: step
        interval: Tokens.durationCountdownStep
        repeat: true
        onTriggered: {
            root.countdown -= 1;
            if (root.countdown > 0)
                return;
            step.stop();
            const pending = root._pending;
            // The picker goes first, so it isn't in the first frames.
            root.picking = false;
            begin.pending = pending;
            begin.restart();
        }
    }

    Timer {
        id: begin
        property var pending: null
        interval: Tokens.pickerGoneDelay
        onTriggered: {
            Daemon.call(Protocol.CMD_START_RECORDING, {
                output: pending.output,
                region: pending.region,
                audio: {
                    desktop: root.desktopAudio,
                    mic: root.micAudio
                }
            }, error => {
                if (error)
                    console.warn(`nova: recording didn't start: ${error}`);
            });
        }
    }

    // A second, for the recording pill's time.
    Timer {
        interval: 1000
        repeat: true
        running: root.recording && !root.active.paused
        onTriggered: root.now = Date.now()
    }

    Connections {
        target: Daemon
        function onRecordingPickRequested() {
            if (root.ready && !root.recording)
                root.picking = !root.picking;
        }
        function onRecordingSaved(saved: var) {
            root.saved(saved);
        }
    }
}
