import QtQuick
import QtQuick.Shapes
import "../theme"

// The capsule's icon, drawn on a 24 px grid and scaled to `size`: a speaker
// with no, one or two waves ("off", "low", "high"), crossed out ("muted"),
// struck through ("none", no output), or headphones.
Item {
    id: icon

    // volume.js icon().
    required property string glyph
    property color color: Tokens.fg
    property int size: Tokens.iconSize

    implicitWidth: size
    implicitHeight: size

    readonly property real grid: 24
    readonly property bool speaker: glyph !== "headphones"
    readonly property string strokes: ({
            off: "",
            low: "M16.5 8.5a5 5 0 0 1 0 7",
            high: "M16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12",
            muted: "M16.5 9.5l5 5M21.5 9.5l-5 5",
            none: "M3 3l18 18",
            headphones: "M4 15v-3a8 8 0 0 1 16 0v3"
        })[glyph] ?? ""
    readonly property string fills: speaker ? "M4 9h4l5-4v14l-5-4H4z" : "M4 15h3.5v6H5.5A1.5 1.5 0 0 1 4 19.5zM20 15h-3.5v6h2a1.5 1.5 0 0 0 1.5-1.5z"

    Shape {
        width: icon.grid
        height: icon.grid
        scale: icon.size / icon.grid
        transformOrigin: Item.TopLeft
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            fillColor: icon.color
            strokeColor: "transparent"
            PathSvg {
                path: icon.fills
            }
        }
        ShapePath {
            fillColor: "transparent"
            strokeColor: icon.color
            strokeWidth: 2
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            PathSvg {
                path: icon.strokes
            }
        }
    }
}
