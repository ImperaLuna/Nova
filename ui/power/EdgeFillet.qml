import QtQuick
import QtQuick.Shapes
import "../theme"

// The concave corner that joins an edge tab to the screen's right edge: a
// square of glass tint (as thick as the tab's beside it, GlassTint) with a
// quarter circle cut from its far corner, and the glass's outline along the
// curve. `above`: it sits on top of the tab,
// curving up into the edge; otherwise below, curving down. A part of the tab
// (StagePanel `parts`): it takes clicks and is blurred like the tab, all but
// its cutout.
Shape {
    id: fillet

    property bool above: true

    // The circle the curve is cut from, centred on the square's corner away
    // from the tab and the edge.
    readonly property Item cutout: Item {
        parent: fillet
        x: -fillet.width
        y: fillet.above ? -fillet.height : 0
        width: 2 * fillet.width
        height: 2 * fillet.height
    }
    // Square: its corners are the tab's and the edge's.
    readonly property int radius: 0

    preferredRendererType: Shape.CurveRenderer

    // Under the outline.
    GlassTint {
        id: tint
        z: -1
        anchors.fill: parent
        cutout: true
        cutCentre: Qt.point(0, fillet.above ? 0 : fillet.height)
        cutRadius: fillet.width
    }

    // The tab's top (or bottom) edge, the screen edge, and the curve that
    // joins them away from the corner they share.
    readonly property real edgeY: above ? height : 0
    readonly property real farY: above ? 0 : height
    ShapePath {
        strokeColor: "transparent"
        fillColor: tint.active ? "transparent" : Tokens.glass
        startX: 0
        startY: fillet.edgeY
        PathLine {
            x: fillet.width
            y: fillet.edgeY
        }
        PathLine {
            x: fillet.width
            y: fillet.farY
        }
        PathArc {
            x: 0
            y: fillet.edgeY
            radiusX: fillet.width
            radiusY: fillet.height
            direction: fillet.above ? PathArc.Clockwise : PathArc.Counterclockwise
        }
    }

    ShapePath {
        strokeColor: Tokens.glassOutline
        strokeWidth: Tokens.glassEdgeWidth
        fillColor: "transparent"
        startX: fillet.width
        startY: fillet.farY
        PathArc {
            x: 0
            y: fillet.edgeY
            radiusX: fillet.width
            radiusY: fillet.height
            direction: fillet.above ? PathArc.Clockwise : PathArc.Counterclockwise
        }
    }
}
