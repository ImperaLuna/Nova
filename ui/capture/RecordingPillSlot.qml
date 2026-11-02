import QtQuick
import "../bar"
import "../theme"
import "service"

// The recording pill in the bar.right row (view `recording.pill`), there
// only while a recording runs.
BarSlot {
    id: slot

    pill: recordingPill
    present: RecordingService.recording
    shapes: [recordingPill]

    RecordingPill {
        id: recordingPill
        visible: slot.present
        x: slot.slotX
        y: Tokens.barGap
        width: implicitWidth
        height: implicitHeight
        elapsed: RecordingService.elapsed
        paused: RecordingService.active?.paused ?? false
        canPause: RecordingService.active?.can_pause ?? false
        onPauseRequested: paused => RecordingService.pause(paused)
        onStopRequested: RecordingService.stop()
    }
}
