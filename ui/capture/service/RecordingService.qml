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
    // The picker was opened by the toolbar's switch.
    property bool switched: false
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
    // The recording didn't start after the countdown: novad's reason, for
    // the card.
    signal failed(string reason)

    onActiveChanged: {
        _since = Date.now();
        now = _since;
    }
    onPickingChanged: {
        mode = Protocol.CAPTURE_REGION;
        if (!picking) {
            switched = false;
            // Frames frozen for the switch go with the picker, unless the
            // switch is showing them.
            if (ScreenshotService.held)
                ScreenshotService.cancel();
        }
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
        _opening = false;
        picking = false;
    }

    // Opens the picker (`nova record`). With screenshots on, the screens
    // are frozen first and the frames held behind it, so the toolbar's
    // switch to a screenshot shows them at once instead of freezing again.
    property bool _opening: false
    function open() {
        if (!ScreenshotService.ready) {
            picking = true;
            return;
        }
        _opening = true;
        ScreenshotService.freeze(error => {
            // Closed again (`nova record` twice) while freezing.
            if (!root._opening) {
                if (!error)
                    ScreenshotService.cancel();
                return;
            }
            root._opening = false;
            if (error)
                console.warn(`nova: recording without a frozen frame to switch to: ${error}`);
            root.picking = true;
        });
    }

    // The card's Try again: the picker once more, with the same sound,
    // unless it's already open or something is recording.
    function again() {
        if (ready && !recording && !picking && !_opening)
            open();
    }

    // Switches to a screenshot in `mode`: the held frames' picker, or a new
    // freeze once this picker has gone.
    function toScreenshot(mode: string) {
        if (ScreenshotService.held && ScreenshotService.frames !== null) {
            ScreenshotService.show(mode);
            picking = false;
        } else {
            picking = false;
            ScreenshotService.pick(mode);
        }
    }

    // Opens the picker in `mode` (the toolbar switched from a screenshot).
    function pick(mode: string) {
        if (!ready || recording)
            return;
        switched = true;
        // The screenshot's frames stay, for switching back.
        ScreenshotService.held = true;
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

    // Opens novad's log, where the whole of a failed start is.
    function openLog() {
        Daemon.call(Protocol.CMD_OPEN_LOG);
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
                if (!error)
                    return;
                console.warn(`nova: recording didn't start: ${error}`);
                root.failed(error);
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
            if (!root.ready || root.recording)
                return;
            if (root.picking || root._opening)
                root.cancel();
            else
                root.open();
        }
        function onRecordingSaved(saved: var) {
            root.saved(saved);
        }
    }
}
