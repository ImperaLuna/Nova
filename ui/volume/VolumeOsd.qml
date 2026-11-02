pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../compositor"
import "../stage"
import "../theme"
import "service"
import "volume.js" as Volume

// The volume capsule: whenever the volume, mute or output changes, it
// slides in from the right edge of the focused screen, and slides away
// Tokens.durationCapsuleHold after the last change (not while the pointer
// is on it). It stays on the screen it came out on until it's gone, and
// stays in on a screen that already shows the volume (quick settings open
// there, VolumeService.shownElsewhere), sliding away if that opens while
// it's out.
Scope {
    id: osd

    // The screen it's out on, or last came out on.
    property ShellScreen screen: null
    // As the capsule lays out by it, kept while that screen goes away.
    readonly property StageScreen output: StageScreen {
        screen: osd.screen
    }
    // The pointer is on it or dragging.
    property bool held: false

    readonly property LiquidMotion motion: LiquidMotion {
        levelTarget: Volume.level(VolumeService.volume, VolumeService.available)
        still: VolumeService.muted || !VolumeService.available
    }

    // Out where it is, or on the focused screen; not where quick settings
    // is open, even while it's still sliding away there.
    function reveal() {
        const target = motion.shown ? screen : Compositor.focusedScreen();
        if (VolumeService.shownElsewhere.includes(target?.name))
            return;
        screen = target;
        motion.openTarget = 1;
        hold.restart();
    }

    Connections {
        target: VolumeService
        function onChanged() {
            osd.reveal();
        }
    }

    // Quick settings opened where it's out: it slides away, as it would
    // never have come out there.
    Connections {
        target: VolumeService
        function onShownElsewhereChanged() {
            if (osd.motion.shown && VolumeService.shownElsewhere.includes(osd.screen?.name)) {
                hold.stop();
                osd.motion.openTarget = 0;
            }
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
            output: osd.output
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

    // `nova volume up|down|mute`, from a volume key bound to Nova
    // (VolumeService acts on it). Shows the capsule even when nothing
    // changes (at either end, or without an output, which it then says).
    Connections {
        target: VolumeService
        function onKeyPressed() {
            osd.reveal();
        }
    }
}
