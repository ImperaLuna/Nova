import QtQuick
import QtQuick.Shapes
import "../theme"
import "../util/numbers.js" as Numbers

// One resource ring: a track, an arc filled to `value` (0 to 1), the
// reading inside it and its name under it.
Item {
    id: ring

    property real value: 0
    required property string reading
    required property string label
    property color color: Tokens.meter

    implicitWidth: Tokens.ringSize
    implicitHeight: Tokens.ringSize + Tokens.iconGap + caption.implicitHeight

    readonly property real middle: Tokens.ringSize / 2
    readonly property real arcRadius: middle - Tokens.ringStroke

    Shape {
        width: Tokens.ringSize
        height: Tokens.ringSize
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            strokeColor: Tokens.track
            strokeWidth: Tokens.ringStroke
            fillColor: "transparent"
            PathAngleArc {
                centerX: ring.middle
                centerY: ring.middle
                radiusX: ring.arcRadius
                radiusY: ring.arcRadius
                sweepAngle: 360
            }
        }
        ShapePath {
            strokeColor: ring.value > 0 ? ring.color : "transparent"
            strokeWidth: Tokens.ringStroke
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            PathAngleArc {
                centerX: ring.middle
                centerY: ring.middle
                radiusX: ring.arcRadius
                radiusY: ring.arcRadius
                startAngle: -90
                sweepAngle: 360 * Numbers.clamp(ring.value, 0, 1)
            }
        }
    }

    Text {
        x: 0
        width: Tokens.ringSize
        height: Tokens.ringSize
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        font.family: Tokens.fontMono
        font.pixelSize: Tokens.fontSizeSmall
        font.weight: Font.Medium
        color: Tokens.fg
        text: ring.reading
    }

    Text {
        id: caption
        y: Tokens.ringSize + Tokens.iconGap
        width: Tokens.ringSize
        horizontalAlignment: Text.AlignHCenter
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeMicro
        font.capitalization: Font.AllUppercase
        font.letterSpacing: Tokens.trackingLabel * Tokens.fontSizeMicro
        color: Tokens.fgSoft
        text: ring.label
    }
}
