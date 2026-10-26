pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import "../theme"
import "quick_settings.js" as QuickSettings

// The resources page's graph: CPU (filled, primary) and memory (secondary)
// over the history novad keeps, newest at the right, with quiet lines at
// a quarter, half and three quarters.
Item {
    id: graph

    // A resources event's `history`: [{cpu, memory}], oldest first.
    property var history: []
    // How many samples fill the width.
    property int span: 1

    implicitHeight: Tokens.graphHeight

    readonly property var cpu: history.map(sample => sample.cpu)
    readonly property var memory: history.map(sample => sample.memory)

    Repeater {
        model: [0.25, 0.5, 0.75]

        Rectangle {
            required property real modelData
            y: graph.height * modelData
            width: graph.width
            height: Tokens.dividerWidth
            color: Tokens.outline
        }
    }

    Shape {
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            strokeColor: "transparent"
            fillColor: Tokens.alpha(Tokens.primary, Tokens.graphAreaOpacity)
            PathSvg {
                path: QuickSettings.graphPath(graph.cpu, graph.width, graph.height, graph.span, true)
            }
        }
        ShapePath {
            strokeColor: Tokens.primary
            strokeWidth: Tokens.graphLine
            fillColor: "transparent"
            joinStyle: ShapePath.RoundJoin
            PathSvg {
                path: QuickSettings.graphPath(graph.cpu, graph.width, graph.height, graph.span, false)
            }
        }
        ShapePath {
            strokeColor: Tokens.secondary
            strokeWidth: Tokens.graphLine
            fillColor: "transparent"
            joinStyle: ShapePath.RoundJoin
            PathSvg {
                path: QuickSettings.graphPath(graph.memory, graph.width, graph.height, graph.span, false)
            }
        }
    }
}
