pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Services.Pipewire
import "../../theme"
import "../volume.js" as Volume

// The default output's volume and mute, from PipeWire, and the commands that
// change them. The capsule (../VolumeOsd.qml) reads and acts through here.
// Media keys stay the compositor's (`wpctl set-volume`): any change shows up
// here, whoever made it.
Singleton {
    id: root

    // The default sink, or null without one (and briefly while it changes).
    readonly property PwNode sink: Pipewire.defaultAudioSink
    readonly property bool available: sink !== null && sink.audio !== null
    // Bound, and its volume known: a node is ready before its channels'
    // volumes arrive, and reads 0 until then.
    readonly property bool ready: available && sink.ready && sink.audio.volumes.length > 0
    // 1 is 100%; another app can set more.
    readonly property real volume: ready ? sink.audio.volume : 0
    readonly property bool muted: ready && sink.audio.muted
    // "none", "headphones" or "speakers" (volume.js outputKind).
    readonly property string output: Volume.outputKind(available, ready ? sink.properties : null)

    // The volume, mute or output changed after Nova had seen where they
    // started, by anything: a key, an app, this capsule.
    signal changed

    // Volume and mute are only valid on a bound node.
    PwObjectTracker {
        objects: root.sink ? [root.sink] : []
    }

    // The last state seen: {id, volume, muted}.
    property var seen: null
    function note() {
        if (!ready)
            return;
        const now = {
            id: sink.id,
            volume: volume,
            muted: muted
        };
        const before = seen;
        seen = now;
        if (before === null)
            // The VM tests wait for this before changing the volume.
            console.info("nova: volume output found");
        else if (Volume.isChange(before, now))
            changed();
    }
    // Once the bindings above have all caught up: a change arrives as
    // several property updates in a row, which would otherwise be seen half
    // done (ready, with the volume still 0).
    onReadyChanged: Qt.callLater(note)
    onVolumeChanged: Qt.callLater(note)
    onMutedChanged: Qt.callLater(note)

    function set(value: real) {
        if (ready)
            sink.audio.volume = value;
    }

    // `direction`: 1 up, -1 down, one Tokens.volumeStep.
    function step(direction: int) {
        set(Volume.stepped(volume, direction, Tokens.volumeStep));
    }

    function toggleMute() {
        if (ready)
            sink.audio.muted = !muted;
    }
}
