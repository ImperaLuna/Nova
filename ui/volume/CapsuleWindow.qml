import QtQuick
import Quickshell
import Quickshell.Wayland
import "../stage"
import "../theme"
import "volume.js" as Volume

// The capsule's own layer surface on the right edge of one screen,
// vertically centered: on the overlay layer, so it shows over fullscreen
// video too, which the stage and PanelSurface (top layer) don't. It's only
// as big as the capsule can get plus its shadow (niri blurs the whole
// surface behind glass), exists only while the capsule is out, takes no
// keyboard and no space, and takes clicks and blur only on the capsule.
PanelWindow { // qmllint disable uncreatable-type
    id: window

    screen: output
    // Its screen, typed (the window's own `screen` isn't, to qmllint).
    required property ShellScreen output
    required property LiquidMotion motion
    required property string kind
    required property real volume
    required property bool muted

    readonly property bool held: capsule.held
    signal stepRequested(int direction)
    signal setRequested(real volume)
    signal muteRequested

    anchors.right: true
    implicitWidth: Tokens.capsuleWidth + Tokens.capsuleEdgeGap + Tokens.shadowRoom
    implicitHeight: Math.ceil(Volume.tallest({
        height: Tokens.capsuleHeight,
        stretch: Tokens.capsuleStretch,
        overshoot: Tokens.capsuleOvershoot
    }) + 2 * Tokens.shadowRoom)
    exclusionMode: ExclusionMode.Ignore
    color: "transparent"
    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    // It moves the whole time it's out, so the other surfaces hold their
    // frames meanwhile (StageMotion). That's at most the hold after a
    // change, but the pointer can hold it out for as long as it likes, so
    // then the others draw again (the bar's clock keeps time).
    readonly property string motionName: `${output.name}/volume`
    readonly property bool holdsOthers: !capsule.held
    onHoldsOthersChanged: StageMotion.setMoving(motionName, holdsOthers)
    Component.onCompleted: {
        StageMotion.setMoving(motionName, holdsOthers);
        BackgroundEffect.blurRegion = blur;
    }
    Component.onDestruction: StageMotion.setMoving(motionName, false)

    mask: Region {
        item: capsule.hit
        radius: capsule.hitRadius
    }
    Region {
        id: blur
        item: capsule.hit
        radius: capsule.hitRadius
    }

    Capsule {
        id: capsule
        anchors.fill: parent
        motion: window.motion
        output: window.kind
        volume: window.volume
        muted: window.muted
        onStepRequested: direction => window.stepRequested(direction)
        onSetRequested: volume => window.setRequested(volume)
        onMuteRequested: window.muteRequested()
    }

    FrameLog {
        surfaceName: window.motionName
    }
}
