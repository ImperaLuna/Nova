import QtQuick
import Quickshell
import Quickshell.Wayland
import "service"

// The region picker's layer surface on one screen: the whole screen, on the
// overlay layer so it covers fullscreen windows and the bar too (the frozen
// frame shows them as they were). It takes the keyboard for Esc, and no
// space. Its own surface, not the stage's: it needs the overlay layer and
// covers everything, so the stage's mask and morph have nothing to add.
PanelWindow { // qmllint disable uncreatable-type
    id: window

    screen: output
    // Its screen, typed (the window's own `screen` isn't, to qmllint).
    required property ShellScreen output
    required property string framePath

    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "nova-capture"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive

    Component.onCompleted: BackgroundEffect.blurRegion = blur
    Region {
        id: blur
        item: picker.toolbarGlass
        radius: picker.toolbarGlass.height / 2
    }

    RegionPicker {
        id: picker
        anchors.fill: parent
        focus: true
        framePath: window.framePath
        mode: ScreenshotService.mode
        busy: ScreenshotService.saving
        onSaveRequested: rect => ScreenshotService.save(window.output.name, rect)
        onCancelRequested: ScreenshotService.cancel()
        onModeRequested: mode => ScreenshotService.mode = mode
        Keys.onPressed: event => {
            event.accepted = picker.key(event.key);
        }
    }
}
