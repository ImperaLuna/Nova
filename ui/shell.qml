//@ pragma UseQApplication

import QtQuick
import Quickshell
import Quickshell.Io
import "compositor"
import "daemon"
import "lock"
import "lock/service"
import "stage"
import "volume"

ShellRoot {
    Variants {
        // Nothing is drawn until novad has sent the theme's colors.
        model: Daemon.theme ? Quickshell.screens : []

        Stage {
            required property var modelData
            screen: modelData
        }
    }

    // Stays loaded while locked even if the feature turns off: dropping the
    // session lock then would leave the screens covered for good.
    LazyLoader {
        active: Daemon.theme !== null && (LockService.ready || LockService.locked)
        component: LockScreen {}
    }

    // The volume capsule, on whichever screen has focus when it comes out.
    LazyLoader {
        active: Daemon.theme !== null
        component: VolumeOsd {}
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
