import QtQuick
import Quickshell
import Quickshell.Wayland
import "../stage"
import "service"
import "capture.js" as Capture

// The region picker's layer surface on one screen: the whole screen, on the
// overlay layer so it covers fullscreen windows and the bar too (the frozen
// frame shows them as they were). It takes the keyboard for Esc, and no
// space. Its own surface, not the stage's: it needs the overlay layer and
// covers everything, so the stage's mask and morph have nothing to add.
// Picking what to record (`video`) it goes through RecordingService, over
// the live screen. The toolbar switches between the two, keeping its mode.
PanelWindow { // qmllint disable uncreatable-type
    id: window

    screen: output.screen
    // Its screen (StageScreen), kept while it goes away.
    required property StageScreen output
    required property string framePath
    property bool video: false
    // Hidden behind the recording picker until its switch shows it.
    property bool _wasHidden: false
    onVisibleChanged: {
        if (!visible)
            _wasHidden = true;
        else if (_wasHidden)
            picker.arriveFrom(!video);
    }

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

    Component.onCompleted: {
        BackgroundEffect.blurRegion = blur;
        if (!visible)
            _wasHidden = true;
    }
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
        video: window.video
        mode: window.video ? RecordingService.mode : ScreenshotService.mode
        busy: window.video ? RecordingService.countdown > 0 : ScreenshotService.saving
        desktopAudio: RecordingService.desktopAudio
        micAudio: RecordingService.micAudio
        countdown: RecordingService.countdown
        canSwitch: ScreenshotService.ready && RecordingService.ready && !RecordingService.recording
        switched: window.video ? RecordingService.switched : ScreenshotService.switched
        onSaveRequested: rect => ScreenshotService.save(window.output.name, rect)
        onRecordRequested: rect => RecordingService.record(window.output.name, rect ? Capture.toLayout(rect, window.output.x, window.output.y) : null)
        onCancelRequested: window.video ? RecordingService.cancel() : ScreenshotService.cancel()
        onModeRequested: mode => {
            if (window.video)
                RecordingService.mode = mode;
            else
                ScreenshotService.mode = mode;
        }
        onVideoRequested: video => {
            if (video)
                RecordingService.pick(ScreenshotService.mode);
            else
                RecordingService.toScreenshot(RecordingService.mode);
        }
        onDesktopAudioToggled: RecordingService.desktopAudio = !RecordingService.desktopAudio
        onMicAudioToggled: RecordingService.micAudio = !RecordingService.micAudio
        Keys.onPressed: event => {
            event.accepted = picker.key(event.key);
        }
    }
}
