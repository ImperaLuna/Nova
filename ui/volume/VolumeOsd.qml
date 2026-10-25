pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../compositor"
import "../daemon"
import "../daemon/protocol.js" as Protocol
import "../theme"
import "service"
import "volume.js" as Volume

// The volume capsule: whenever the volume, mute or output changes, it
// slides in from the right edge of the focused screen, and slides away
// Tokens.durationCapsuleHold after the last change (not while the pointer
// is on it). It stays on the screen it came out on until it's gone, and
// stays in on a screen that already shows the volume (quick settings open
// there, VolumeService.shownElsewhere).
Scope {
    id: osd

    // The screen it's out on, or last came out on.
    property ShellScreen screen: null
    // The pointer is on it or dragging.
    property bool held: false

    readonly property LiquidMotion motion: LiquidMotion {
        levelTarget: Volume.level(VolumeService.volume, VolumeService.available)
        still: VolumeService.muted || !VolumeService.available
    }

    function reveal() {
        if (!motion.shown) {
            const focused = focusedScreen();
            if (VolumeService.shownElsewhere.includes(focused?.name))
                return;
            screen = focused;
        }
        motion.openTarget = 1;
        hold.restart();
    }

    function focusedScreen(): ShellScreen {
        const name = Compositor.focusedOutput?.name;
        return Quickshell.screens.find(screen => screen.name === name) ?? Quickshell.screens[0] ?? null;
    }

    Connections {
        target: VolumeService
        function onChanged() {
            osd.reveal();
        }
    }

    Timer {
        id: hold
        interval: Tokens.durationCapsuleHold
        onTriggered: {
            if (!osd.held)
                osd.motion.openTarget = 0;
        }
    }
    onHeldChanged: {
        if (!held)
            hold.restart();
    }

    readonly property bool shown: motion.shown
    onShownChanged: {
        // The VM tests wait for these.
        console.info(`nova: volume capsule ${shown ? "shown" : "hidden"}`);
    }

    LazyLoader {
        active: osd.screen !== null && osd.motion.shown
        component: CapsuleWindow {
            output: osd.screen
            motion: osd.motion
            kind: VolumeService.output
            volume: VolumeService.volume
            muted: VolumeService.muted
            onHeldChanged: osd.held = held
            onStepRequested: direction => VolumeService.step(direction)
            onSetRequested: volume => VolumeService.set(volume)
            onMuteRequested: VolumeService.toggleMute()
            Component.onDestruction: osd.held = false
        }
    }

    // `nova volume up|down|mute`, from a volume key bound to Nova. Shows the
    // capsule even when nothing changes (at either end, or without an
    // output, which it then says).
    Connections {
        target: Daemon
        function onVolumeRequested(key: string) {
            if (key === Protocol.VOLUME_UP)
                VolumeService.step(1);
            else if (key === Protocol.VOLUME_DOWN)
                VolumeService.step(-1);
            else if (key === Protocol.VOLUME_MUTE)
                VolumeService.toggleMute();
            else {
                console.warn(`nova: unknown volume key ${key}`);
                return;
            }
            osd.reveal();
        }
    }
}
