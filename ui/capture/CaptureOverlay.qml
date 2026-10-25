pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../stage"
import "service"
import "capture.js" as Capture

// The region picker on every screen novad froze, while it's open
// (`nova screenshot region`). It goes as soon as a pick is sent, so the
// screens are live again while novad saves it.
Scope {
    Variants {
        model: ScreenshotService.picking && !ScreenshotService.saving ? Quickshell.screens.filter(screen => Capture.frameFor(ScreenshotService.frames, screen.name) !== null) : []

        CaptureWindow {
            id: window
            required property ShellScreen modelData
            output: StageScreen {
                screen: window.modelData
            }
            framePath: Capture.frameFor(ScreenshotService.frames, output.name)?.path ?? ""
        }
    }
}
