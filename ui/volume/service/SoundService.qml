pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Services.Pipewire
import "../volume.js" as Volume
import "../../quick_settings/quick_settings.js" as QuickSettings

// Everything quick settings shows about sound beyond the default output
// (VolumeService has that): the default input and who records from it,
// every output and input to switch between, and each app's own volume.
// Lists come as plain objects ({id, name, kind, volume, muted, ready,
// icon, current};
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
            ready: node.ready && node.audio.volumes.length > 0,
            icon: appIcon(props),
            current: current !== null && node.id === current.id
        };
    }

    // The installed apps' desktop entries, as plain objects for
    // quick_settings.js entryIcon. They load a moment after start, and the
    // lists above follow.
    // qmllint disable missing-property
    readonly property var entries: DesktopEntries.applications.values.map(entry => ({
                id: entry.id,
                name: entry.name,
                startupClass: entry.startupClass,
                command: entry.command,
                icon: entry.icon
            }))
    // qmllint enable missing-property

    // An app stream's icon name: its desktop entry's, found by the icon name
    // the stream gives (often the entry's id, like Claude's
    // `com.anthropic.Claude`), its program or its name (browsers give no
    // icon), else the stream's icon name or program, in case the icon theme
    // has it.
    function appIcon(props: var): string {
        const given = props["application.icon-name"] ?? "";
        const program = QuickSettings.programName(props["application.process.binary"] ?? "");
        return QuickSettings.entryIcon(entries, [given, program, props["application.name"] ?? ""]) || given || program;
    }

    function node(id: int): PwNode {
        return nodes.find(node => node.id === id) ?? null;
    }

    // Moved by hand, so unmuted as well, like the output.
    function setVolume(id: int, value: real) {
        const found = node(id);
        if (!found?.ready)
            return;
        found.audio.volume = value;
        if (found.audio.muted)
            found.audio.muted = false;
    }

    // Every stream of an app (an `apps` entry's `ids`) to `value`.
    function setVolumes(ids: list<int>, value: real) {
        for (const id of ids)
            setVolume(id, value);
    }

    // Every stream of an app to `muted`.
    function setMuted(ids: list<int>, muted: bool) {
        for (const id of ids) {
            const found = node(id);
            if (found?.ready)
                found.audio.muted = muted;
        }
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
        if (!sourceReady)
            return;
        source.audio.volume = value;
        if (sourceMuted)
            source.audio.muted = false;
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
