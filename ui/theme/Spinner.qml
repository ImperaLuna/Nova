import QtQuick
import QtQuick.Shapes

// Nova's loading spinner: an arc going round in `color`, while `spinning`
// and in sight. `track` draws the whole ring under it (dashed with
// `dashed`), for a ring that stays when it stops.
Shape {
    id: spinner

    property color color: Tokens.fg
    property int size: Tokens.iconSize
    property real radius: size / 2 - Tokens.ringInset
    property bool spinning: true
    property color track: "transparent"
    property bool dashed: false
    // Going round now: it asks for frames.
    readonly property bool turning: spin.running

    width: size
    height: size
    preferredRendererType: Shape.CurveRenderer

    ShapePath {
        strokeColor: spinner.track
        strokeWidth: spinner.track.a > 0 ? Tokens.strokeThin : 0
        fillColor: "transparent"
        strokeStyle: spinner.dashed ? ShapePath.DashLine : ShapePath.SolidLine
        dashPattern: Tokens.ringDashes
        PathAngleArc {
            centerX: spinner.size / 2
            centerY: spinner.size / 2
            radiusX: spinner.radius
            radiusY: spinner.radius
            sweepAngle: 360
        }
    }

    ShapePath {
        strokeColor: spinner.color
        strokeWidth: spinner.spinning ? Tokens.strokeThin : 0
        fillColor: "transparent"
        capStyle: ShapePath.RoundCap
        PathAngleArc {
            centerX: spinner.size / 2
            centerY: spinner.size / 2
            radiusX: spinner.radius
            radiusY: spinner.radius
            sweepAngle: Tokens.spinnerSweep
        }
    }

    RotationAnimator on rotation {
        id: spin
        // Only while it can be seen: a hidden or faded-out one costs no frames.
        running: spinner.spinning && spinner.visible && spinner.opacity > 0
        from: 0
        to: 360
        duration: Tokens.durationSpin
        loops: Animation.Infinite
    }
}
