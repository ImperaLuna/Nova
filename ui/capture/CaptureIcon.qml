import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol
import "capture.js" as Capture

// The capture toolbar's and the screenshot card's line icons, stroked in
// `color`: a Protocol.CAPTURE_* mode, a Protocol.SHOT_* action, or a
// Capture.ICON_* name.
LineIcon {
    required property string name

    path: ({
            [Protocol.CAPTURE_REGION]: Icons.REGION,
            [Protocol.CAPTURE_SCREEN]: Icons.MONITOR,
            [Protocol.SHOT_OPEN]: Icons.OPEN,
            [Protocol.SHOT_FOLDER]: Icons.FOLDER,
            [Protocol.SHOT_DELETE]: Icons.TRASH,
            [Capture.ICON_CAMERA]: Icons.CAMERA,
            [Capture.ICON_CLOSE]: Icons.CLOSE,
            [Capture.ICON_CHECK]: Icons.CHECK,
            [Capture.ICON_DESKTOP_AUDIO]: Icons.SPEAKER,
            [Capture.ICON_MIC]: Icons.MIC,
            [Capture.ICON_PAUSE]: Icons.PAUSE,
            [Capture.ICON_RESUME]: Icons.PLAY,
            [Capture.ICON_STOP]: Icons.STOP,
            [Capture.ICON_VIDEO]: Icons.VIDEO,
            [Capture.ICON_VIDEO_OFF]: Icons.VIDEO_OFF,
            [Capture.CARD_RETRY]: Icons.REBOOT,
            [Capture.CARD_LOG]: Icons.DOCUMENT
        })[name] ?? ""
}
