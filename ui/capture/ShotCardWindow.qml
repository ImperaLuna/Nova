import QtQuick
import Quickshell
import Quickshell.Wayland
import "../stage"
import "../theme"
import "service"
import "capture.js" as Capture

// The screenshot (or recording) card's own layer surface in the bottom-right corner of one
// screen: on the overlay layer like the volume capsule, so it shows over
// fullscreen windows too, only as big as the card plus its shadow, taking
// no keyboard and no space, and clicks and blur only on the card. It slides
// in from the edge, and out before it goes.
PanelWindow { // qmllint disable uncreatable-type
    id: window

    readonly property CoversGlass cover: CoversGlass {
        shown: window.visible
    }

    screen: output.screen
    // Its screen (StageScreen), kept while it goes away.
    required property StageScreen output
    required property var shot
    required property bool video
    // False slides the card out; `gone` follows once it's out.
    property bool shown: true
    signal gone

    anchors.right: true
    anchors.bottom: true
    implicitWidth: card.implicitWidth + Tokens.shotCardGap + Tokens.shadowRoom
    implicitHeight: card.implicitHeight + Tokens.shotCardGap + Tokens.shadowRoom
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "nova-screenshot"
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    // A new shot while it's out: back in, with the full time.
    onShotChanged: {
        shown = true;
        card.restart();
    }

    Component.onCompleted: {
        BackgroundEffect.blurRegion = blur;
        slide.out = false;
    }
    mask: Region {
        item: card
        radius: Tokens.radiusPanel
    }
    Region {
        id: blur
        item: card
        radius: Tokens.radiusPanel
    }

    QtObject {
        id: slide
        property bool out: true
    }

    ShotCard {
        id: card
        x: Tokens.shadowRoom + ((slide.out || !window.shown) ? width + Tokens.shotCardGap : 0)
        y: Tokens.shadowRoom
        opacity: (slide.out || !window.shown) ? 0 : 1
        shot: window.shot
        video: window.video
        onActionRequested: action => {
            if (action === Capture.CARD_RETRY)
                RecordingService.again();
            else if (action === Capture.CARD_LOG)
                RecordingService.openLog();
            else if (window.video)
                RecordingService.act(window.shot.id, action);
            else
                ScreenshotService.act(window.shot.id, action);
            window.shown = false;
        }
        onExpired: window.shown = false

        Behavior on x {
            NumberAnimation {
                id: move
                duration: Tokens.durationPanel
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
                onRunningChanged: {
                    if (!running && !window.shown)
                        window.gone();
                }
            }
        }
        Behavior on opacity {
            NumberAnimation {
                duration: Tokens.durationPanel
            }
        }
    }

    FrameLog {
        surfaceName: `${window.output.name}/screenshot`
    }
}
