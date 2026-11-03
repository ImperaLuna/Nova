import QtQuick
import QtQuick.Shapes

// A line icon from an SVG path on a 24-unit grid (icons.js has Nova's),
// stroked in `color` and scaled to `size`; `stroke` is the line's width on
// that grid, so a big icon can keep its lines fine. `fill` is drawn solid
// underneath, for the few icons with a solid part.
Item {
    id: icon

    required property string path
    property string fill: ""
    property color color: Tokens.fg
    property int size: Tokens.iconSize
    property real stroke: Tokens.iconStroke

    implicitWidth: size
    implicitHeight: size

    readonly property real grid: 24

    Shape {
        width: icon.grid
        height: icon.grid
        scale: icon.size / icon.grid
        transformOrigin: Item.TopLeft
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            fillColor: icon.fill === "" ? "transparent" : icon.color
            strokeColor: "transparent"
            PathSvg {
                path: icon.fill
            }
        }
        ShapePath {
            fillColor: "transparent"
            strokeColor: icon.color
            strokeWidth: icon.stroke
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            PathSvg {
                path: icon.path
            }
        }
    }
}
