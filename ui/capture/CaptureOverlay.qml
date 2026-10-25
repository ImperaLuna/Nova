pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../stage"
import "service"
import "capture.js" as Capture

// The region picker on every screen novad froze, while it's open
// (`nova screenshot region`). It goes as soon as a pick is sent, so the
// screens are live again while novad saves it. Picking what to record
// (`nova record`) it's on every screen, over the live screen, and the
// corner marks show around a region while it's recorded.
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

    Variants {
        model: RecordingService.picking ? Quickshell.screens : []

        CaptureWindow {
            id: recordWindow
            required property ShellScreen modelData
            output: StageScreen {
                screen: recordWindow.modelData
            }
            framePath: ""
            video: true
        }
    }

    Variants {
        model: RecordingService.active?.region ? Quickshell.screens.filter(screen => screen.name === RecordingService.active.output) : []

        RecordingMarks {
            id: marks
            required property ShellScreen modelData
            output: StageScreen {
                screen: marks.modelData
            }
            region: RecordingService.active?.region ?? Qt.rect(0, 0, 0, 0)
        }
    }
}
