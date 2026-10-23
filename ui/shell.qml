//@ pragma UseQApplication

import QtQuick
import Quickshell
import Quickshell.Io
import "bar"
import "compositor"
import "daemon"

ShellRoot {
    Variants {
        // Nothing is drawn until novad has sent the theme's colors.
        model: Daemon.theme ? Quickshell.screens : []

        Bar {
            required property var modelData
            screen: modelData
        }
    }

    // `quickshell ipc --pid <UI pid> prop get compositor focusedWorkspace`:
    // what the UI got from novad, for the VM tests and scripts.
    IpcHandler {
        target: "compositor"
        readonly property string focusedWorkspace: Compositor.focusedWorkspace?.name ?? ""
    }
}
