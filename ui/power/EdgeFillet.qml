import QtQuick
import QtQuick.Shapes
import "../theme"

// The concave corner that joins an edge tab to the screen's right edge: a
// square of glass tint with a quarter circle cut from its far corner, and
// the rim along the curve. `above`: it sits on top of the tab, curving up
// into the edge; otherwise below, curving down.
Shape {
    id: fillet

    property bool above: true

    preferredRendererType: Shape.CurveRenderer

    // The tab's top (or bottom) edge, the screen edge, and the curve that
    // joins them away from the corner they share.
    readonly property real edgeY: above ? height : 0
    readonly property real farY: above ? 0 : height
    // The tab's glass where the fillet meets it: its sheen at the top, its
    // shade at the bottom (Glass), over the tint.
    readonly property color fill: over(above ? Tokens.glassSheen : Tokens.glassShade, Tokens.glass)

    // `top` composited over `under`, both translucent.
    function over(top: color, under: color): color {
        const alpha = top.a + under.a * (1 - top.a);
        if (alpha === 0)
            return "transparent";
        const mix = (a, b) => (a * top.a + b * under.a * (1 - top.a)) / alpha;
        return Qt.rgba(mix(top.r, under.r), mix(top.g, under.g), mix(top.b, under.b), alpha);
    }

    ShapePath {
        strokeColor: "transparent"
        fillColor: fillet.fill
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
        strokeColor: Tokens.glassRim
        strokeWidth: Tokens.glassRimWidth
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
