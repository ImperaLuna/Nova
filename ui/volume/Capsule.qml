import QtQuick
import QtQuick.Shapes
import "../theme"
import "volume.js" as Volume
import "../util/numbers.js" as Numbers

// The capsule as drawn, in a window as big as it can get: the glass, liquid
// and shadow in one shader (liquid.frag), the icon and number over it, and
// a dashed outline when there's no output. Scrolling on it steps the
// volume, dragging or clicking sets it, clicking the icon mutes.
Item {
    id: capsule

    required property LiquidMotion motion
    // volume.js outputKind(), the volume (1 is 100%) and mute.
    required property string output
    required property real volume
    required property bool muted

    // The pointer is on it or dragging: it stays out.
    readonly property bool held: input.containsMouse || input.pressed
    // Where it's drawn this frame, and the part that takes input and blur.
    readonly property var box: Volume.box(motion.open, motion.openSpeed, width, height, {
        width: Tokens.capsuleWidth,
        height: Tokens.capsuleHeight,
        gap: Tokens.capsuleEdgeGap,
        stretch: Tokens.capsuleStretch,
        stretchSpeed: Tokens.capsuleStretchSpeed,
        overshoot: Tokens.capsuleOvershoot
    })
    readonly property Item hit: hit
    readonly property real hitRadius: box.radius

    signal stepRequested(int direction)
    signal setRequested(real volume)
    signal muteRequested

    readonly property bool unavailable: output === "none"

    ShaderEffect {
        anchors.fill: parent

        readonly property size size: Qt.size(width, height)
        readonly property rect box: Qt.rect(capsule.box.x, capsule.box.y, capsule.box.w, capsule.box.h)
        readonly property real radius: capsule.box.radius
        readonly property real wall: Tokens.capsuleWall
        readonly property real level: capsule.motion.level
        readonly property real time: capsule.motion.time
        readonly property real rise: capsule.motion.rise
        property real swell: capsule.motion.still ? 0 : Tokens.liquidSwell
        Behavior on swell {
            NumberAnimation {
                duration: Tokens.durationColor
            }
        }
        readonly property real amp: capsule.motion.amp
        readonly property real tilt: capsule.motion.tilt
        readonly property real meniscus: Tokens.liquidMeniscus
        readonly property real meniscusReach: Tokens.liquidMeniscusReach
        readonly property real fizz: capsule.motion.fizz
        readonly property real specks: Tokens.liquidSpecks
        readonly property real speckRadius: Tokens.liquidSpeckRadius
        readonly property real speckRadiusRange: Tokens.liquidSpeckRadiusRange
        readonly property real speckGlow: Tokens.liquidSpeckGlow
        readonly property real speckGlowMax: Tokens.liquidSpeckGlowMax
        readonly property real glow: Tokens.liquidGlow
        readonly property real glowDepth: Tokens.liquidGlowDepth
        readonly property real streak: Tokens.liquidStreak
        readonly property real streakFade: Tokens.liquidStreakFade
        readonly property real streakAt: Tokens.liquidStreakAt
        readonly property real streakWidth: Tokens.liquidStreakWidth
        readonly property real rimOn: capsule.unavailable ? 0 : 1
        readonly property real rimWidth: Tokens.glassRimWidth
        readonly property real rimShade: Tokens.glassRimShade
        readonly property real sheenFade: Tokens.glassVeilAt
        readonly property real shadowBlur: Tokens.shadowBlur
        readonly property real shadowOffset: Tokens.shadowOffset
        readonly property color tint: Tokens.glass
        readonly property color sheen: Tokens.glassSheen
        readonly property color rimLit: Tokens.glassRimLit
        property color liquidColor: capsule.muted ? Tokens.liquidMuted : Tokens.liquid
        Behavior on liquidColor {
            ColorAnimation {
                duration: Tokens.durationColor
            }
        }
        readonly property color shadow: Tokens.shadow
        readonly property color light: Tokens.liquidLight

        fragmentShader: Qt.resolvedUrl("liquid.frag.qsb")
    }

    // Input, blur and the label follow the capsule.
    Item {
        id: hit
        readonly property real radius: capsule.box.radius
        x: capsule.box.x
        y: capsule.box.y
        width: capsule.box.w
        height: capsule.box.h

        // No output: a dashed outline instead of the lit rim.
        Shape {
            anchors.fill: parent
            visible: capsule.unavailable
            preferredRendererType: Shape.CurveRenderer
            ShapePath {
                fillColor: "transparent"
                strokeColor: Tokens.glassRimGlow
                strokeWidth: Tokens.strokeThin
                strokeStyle: ShapePath.DashLine
                dashPattern: Tokens.ringDashes
                PathRectangle {
                    x: Tokens.strokeThin / 2
                    y: Tokens.strokeThin / 2
                    width: hit.width - Tokens.strokeThin
                    height: hit.height - Tokens.strokeThin
                    radius: hit.radius - Tokens.strokeThin / 2
                }
            }
        }

        CapsuleLabel {
            id: label
            anchors.fill: parent
            // Fades in over the last stretch of coming out.
            opacity: Numbers.clamp(1 - (1 - capsule.motion.open) / Tokens.capsuleLabelFadeIn, 0, 1)
            glyph: Volume.icon(capsule.output, capsule.muted, capsule.volume)
            text: capsule.unavailable ? "–" : String(Volume.percent(capsule.volume))
            dimmed: capsule.muted
            surfaceY: Volume.surfaceY(capsule.motion.level, hit.height, Tokens.capsuleWall)
        }

        MouseArea {
            id: input
            anchors.fill: parent
            hoverEnabled: true
            enabled: !capsule.unavailable
            cursorShape: Qt.SizeVerCursor

            // Wheel deltas add up to whole notches (touchpads send small ones).
            property real wheel: 0
            property real pressY: 0
            property bool dragged: false

            function fractionAt(y: real): real {
                return 1 - (y - Tokens.capsuleWall) / (height - 2 * Tokens.capsuleWall);
            }

            onWheel: event => {
                wheel += event.angleDelta.y;
                while (Math.abs(wheel) >= Tokens.wheelNotch) {
                    const direction = Math.sign(wheel);
                    wheel -= direction * Tokens.wheelNotch;
                    capsule.stepRequested(direction);
                }
            }
            onPressed: mouse => {
                pressY = mouse.y;
                dragged = false;
            }
            onPositionChanged: mouse => {
                if (!pressed)
                    return;
                if (Math.abs(mouse.y - pressY) > Application.styleHints.startDragDistance)
                    dragged = true;
                if (dragged)
                    capsule.setRequested(Volume.atFraction(fractionAt(mouse.y)));
            }
            onClicked: mouse => {
                if (dragged)
                    return;
                const face = label.face;
                if (mouse.y >= face.y && mouse.y <= face.y + face.height)
                    capsule.muteRequested();
                else
                    capsule.setRequested(Volume.atFraction(fractionAt(mouse.y)));
            }
        }
    }
}
