pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Services.Pipewire
import "../volume.js" as Volume
import "../../quick_settings/quick_settings.js" as QuickSettings

// Everything quick settings shows about sound beyond the default output
// (VolumeService has that): the default input and who records from it,
// every output and input to switch between, and each app's own volume.
// Lists come as plain objects ({id, name, kind, volume, muted, current};
// apps also have `ids`),
// so the pages showing them don't need PipeWire's types; acting goes
// through the functions here by id.
Singleton {
    id: root

    // The default source (the mic), or null without one.
    readonly property PwNode source: Pipewire.defaultAudioSource
    readonly property bool sourceReady: source !== null && source.audio !== null && source.ready && source.audio.volumes.length > 0
    readonly property real sourceVolume: sourceReady ? source.audio.volume : 0
    readonly property bool sourceMuted: sourceReady && source.audio.muted
    readonly property string sourceName: source ? (source.nickname || source.description || source.name) : ""

    // Apps recording from the default source now, by name.
    readonly property list<string> recorders: QuickSettings.recorders(source, recording.linkGroups)

    // Whether the lists below are wanted (the sound page is open): only
    // then are all their nodes bound, for their volumes.
    property bool listing: false

    // qmllint disable missing-property
    readonly property list<PwNode> nodes: Pipewire.nodes.values.filter(node => node.audio !== null)
    // qmllint enable missing-property
    readonly property list<PwNode> outputNodes: nodes.filter(node => node.isSink && !node.isStream)
    readonly property list<PwNode> inputNodes: nodes.filter(node => !node.isSink && !node.isStream)
    readonly property list<PwNode> appNodes: nodes.filter(node => node.isSink && node.isStream)

    readonly property var outputs: outputNodes.map(node => describe(node, Pipewire.defaultAudioSink))
    readonly property var inputs: inputNodes.map(node => describe(node, root.source))
    // One entry per app, with all its streams' `ids` (quick_settings.js
    // appGroups).
    readonly property var apps: QuickSettings.appGroups(appNodes.map(node => describe(node, null)))

    function describe(node: PwNode, current: PwNode): var {
        const props = node.properties ?? {};
        return {
            id: node.id,
            name: props["application.name"] || node.nickname || node.description || node.name,
            kind: Volume.outputKind(true, props),
            volume: node.ready && node.audio.volumes.length > 0 ? node.audio.volume : 0,
            muted: node.ready && node.audio.muted,
            current: current !== null && node.id === current.id
        };
    }

    function node(id: int): PwNode {
        return nodes.find(node => node.id === id) ?? null;
    }

    function setVolume(id: int, value: real) {
        const found = node(id);
        if (found?.ready)
            found.audio.volume = value;
    }

    // Every stream of an app (an `apps` entry's `ids`) to `value`.
    function setVolumes(ids: list<int>, value: real) {
        for (const id of ids)
            setVolume(id, value);
    }

    function useOutput(id: int) {
        const found = node(id);
        if (found)
            Pipewire.preferredDefaultAudioSink = found;
    }

    function useInput(id: int) {
        const found = node(id);
        if (found)
            Pipewire.preferredDefaultAudioSource = found;
    }

    function setSourceVolume(value: real) {
        if (sourceReady)
            source.audio.volume = value;
    }

    function toggleSourceMute() {
        if (sourceReady)
            source.audio.muted = !sourceMuted;
    }

    PwNodeLinkTracker {
        id: recording
        node: root.source
    }

    PwObjectTracker {
        objects: root.listing ? root.nodes : (root.source ? [root.source] : [])
    }
}
