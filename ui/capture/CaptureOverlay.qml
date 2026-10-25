pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "service"
import "capture.js" as Capture

// The region picker on every screen novad froze, while it's open
// (`nova screenshot region`).
Scope {
    Variants {
        model: ScreenshotService.picking ? Quickshell.screens.filter(screen => Capture.frameFor(ScreenshotService.frames, screen.name) !== null) : []

        CaptureWindow {
            required property ShellScreen modelData
            output: modelData
            framePath: Capture.frameFor(ScreenshotService.frames, modelData.name)?.path ?? ""
        }
    }
}
