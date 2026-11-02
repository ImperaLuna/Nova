pragma ComponentBehavior: Bound

//@ pragma UseQApplication

import QtQuick
import Quickshell
import Quickshell.Io
import "compositor"
import "daemon"
import "stage"
import "volume/service"

ShellRoot {
    Variants {
        id: stages
        // Nothing is drawn until novad has sent the theme's colors.
        model: Daemon.theme ? Quickshell.screens : []

        Stage {
            required property var modelData
            screen: modelData
            catchesAnywhere: stages.instances.some(stage => (stage as Stage)?.catchesClicks)
        }
    }

    // The volume capsule, the lock screen, notification pop-ups, the
    // capture overlay and pairing questions: the shell.overlays host.
    OverlaySlots {}

    // Volume keys bound to `nova volume`, whatever draws the volume (the
    // capsule, quick settings or neither).
    Connections {
        target: Daemon
        function onVolumeRequested(key: string) {
            VolumeService.pressKey(key);
        }
    }

    // `quickshell ipc --pid <UI pid> prop get compositor focusedWorkspace`:
    // what the UI got from novad, for the VM tests and scripts.
    IpcHandler {
        target: "compositor"
        readonly property string focusedWorkspace: Compositor.focusedWorkspace?.name ?? ""
    }

    // `quickshell ipc --pid <UI pid> call demo toggle <name>`: `nova toggle`
    // for the placeholder panels (NOVA_DEMO_PANELS=1, ui/demo), which aren't
    // features novad knows. On the focused screen, like `nova toggle`.
    IpcHandler {
        target: "demo"
        function toggle(panel: string): void {
            Daemon.toggleRequested(panel, Compositor.focusedOutput?.name ?? null);
        }
    }
}
