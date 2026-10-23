pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import "../theme"

// A provider's mark, drawn on a 24-unit grid in the theme's foreground.
Item {
    id: logo

    required property string provider
    property color color: Tokens.fg

    readonly property real unit: width / 24

    implicitWidth: 14
    implicitHeight: implicitWidth

    // Claude: an open A with a parallel stroke.
    Shape {
        anchors.fill: parent
        visible: logo.provider === "claude"
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            strokeColor: logo.color
            strokeWidth: 2.6 * logo.unit
            fillColor: "transparent"
            scale: Qt.size(logo.unit, logo.unit)
            PathSvg {
                path: "M 2 21 L 8 3 L 14 21 M 4.6 14.5 L 11.4 14.5 M 14 3 L 20 21"
            }
        }
    }

    // Codex: three rounded petals, 60 degrees apart.
    Repeater {
        model: logo.provider === "codex" ? 3 : 0

        Rectangle {
            required property int index
            x: 8.5 * logo.unit
            y: 2.5 * logo.unit
            width: 7 * logo.unit
            height: 19 * logo.unit
            radius: width / 2
            rotation: index * 60
            color: "transparent"
            border.color: logo.color
            border.width: 2 * logo.unit
            antialiasing: true
        }
    }
}
