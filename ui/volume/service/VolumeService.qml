pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Services.Pipewire
import "../../theme"
import "../volume.js" as Volume

// The default output's volume and mute, from PipeWire, and the commands that
// change them. The capsule (../VolumeOsd.qml) reads and acts through here.
// Volume keys bound to `nova volume` arrive through Daemon; keys left on
// the compositor (`wpctl set-volume`) work too: any change shows up here,
// whoever made it.
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
    // "none", "headphones", "bluetooth", "display" or "speakers" (volume.js
    // outputKind).
    readonly property string output: Volume.outputKind(available, ready ? sink.properties : null)
    // The default output's name as people know it, or "" without one.
    readonly property string outputName: sink ? (sink.nickname || sink.description || sink.name) : ""

    // Screens (by name) showing the volume somewhere else right now, like
    // open quick settings: the capsule stays in there, since it would only
    // repeat it and hold that surface's frames while it's out
    // (StageMotion).
    property list<string> shownElsewhere: []
    function showElsewhere(screen: string, shown: bool) {
        const others = shownElsewhere.filter(name => name !== screen);
        shownElsewhere = shown ? [...others, screen] : others;
    }

    // The volume, mute or output changed after Nova had seen where they
    // started, by anything: a key, an app, this capsule.
    signal changed

    // A Bluetooth output or mic that appears becomes the default (volume.js
    // arrivedBluetooth); one there when Nova starts is left as WirePlumber
    // chose: the nodes seen once PipeWire's first listing is done (nodes
    // come one by one until it's ready) are the start. Going back to the
    // dongle needs nothing: the Bluetooth node goes and PipeWire falls back.
    // A Bluetooth mic in use puts the headset in call mode (mono sound)
    // until it's let go; the user chose that over a dongle mic that may be
    // silent.
    // qmllint disable missing-property
    readonly property list<PwNode> outputs: Pipewire.nodes.values.filter(node => node.isSink && !node.isStream && node.audio !== null)
    readonly property list<PwNode> inputs: Pipewire.nodes.values.filter(node => !node.isSink && !node.isStream && node.audio !== null)
    // qmllint enable missing-property
    property var knownOutputs: null
    property var knownInputs: null
    onOutputsChanged: followOutputs()
    onInputsChanged: followInputs()
    readonly property bool listed: Pipewire.ready
    onListedChanged: {
        followOutputs();
        followInputs();
    }
    function followOutputs() {
        const node = arrived(outputs, knownOutputs);
        if (listed)
            knownOutputs = outputs.map(node => node.id);
        if (node) {
            console.info(`nova: switching sound to ${node.description || node.name}`);
            Pipewire.preferredDefaultAudioSink = node;
        }
    }
    function followInputs() {
        const node = arrived(inputs, knownInputs);
        if (listed)
            knownInputs = inputs.map(node => node.id);
        if (node) {
            console.info(`nova: switching the mic to ${node.description || node.name}`);
            Pipewire.preferredDefaultAudioSource = node;
        }
    }
    // The Bluetooth node among `nodes` that `known` didn't have, or null
    // before PipeWire's first listing is done.
    function arrived(nodes: var, known: var): var {
        if (!listed || known === null)
            return null;
        const id = Volume.arrivedBluetooth(known, nodes.map(node => ({
                    id: node.id,
                    name: node.name
                })));
        return nodes.find(node => node.id === id) ?? null;
    }

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

    // A slider moved to `value`: moving it means wanting to hear it, so it
    // unmutes too.
    function slide(value: real) {
        set(value);
        if (muted)
            sink.audio.muted = false;
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
