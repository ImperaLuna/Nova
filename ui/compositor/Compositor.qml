pragma Singleton

import QtQuick
import Quickshell
import "../daemon"
import "../daemon/protocol.js" as Protocol

// The compositor's workspaces, focused window and keyboard layout, the same
// on every compositor. Check `has(capability)` before showing something;
// never which compositor it is.
Singleton {
    id: root

    // Null until novad has heard from the compositor, and on compositors it
    // doesn't know: everything below is then empty.
    readonly property var desktop: Daemon.desktop
    readonly property var outputs: desktop?.outputs ?? []
    readonly property var focusedWindow: desktop?.focused_window ?? null
    readonly property string keyboardLayout: desktop?.keyboard_layout ?? ""
    readonly property var focusedWorkspace: Protocol.focusedWorkspace(desktop)
    // The output showing the focused workspace, or null.
    readonly property var focusedOutput: outputs.find(output => output.workspaces.some(workspace => workspace.focused)) ?? null

    function has(capability: string): bool {
        return desktop?.capabilities.includes(capability) ?? false;
    }

    // The output Quickshell calls `screen.name`, or null.
    function output(name: string): var {
        return outputs.find(output => output.name === name) ?? null;
    }

    // The screen with focus, for something that comes out on one screen
    // (the volume capsule, the screenshot card): the first one when novad
    // doesn't know which.
    function focusedScreen(): ShellScreen {
        const name = focusedOutput?.name;
        return Quickshell.screens.find(screen => screen.name === name) ?? Quickshell.screens[0] ?? null;
    }

    function focusWorkspace(id: int) {
        Daemon.call(Protocol.CMD_FOCUS_WORKSPACE, {
            id: id
        });
    }

    // `step` is Protocol.STEP_NEXT or STEP_PREVIOUS.
    function stepWorkspace(outputName: string, step: string) {
        Daemon.call(Protocol.CMD_STEP_WORKSPACE, {
            output: outputName,
            step: step
        });
    }
}
