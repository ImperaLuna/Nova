import QtQuick
import QtQuick.Shapes
import "../theme"

// The launcher's line icons, stroked in `color` on a 24-unit grid like the
// power menu's: "search" (a magnifier, also on the central pill) and "pin"
// (a pushpin, filled once the app is pinned).
Item {
    id: icon

    required property string name
    property color color: Tokens.fg
    // Fills the shape too, like a pinned app's pin.
    property bool filled: false

    readonly property var paths: ({
            search: "M10.5 4a6.5 6.5 0 1 1 0 13a6.5 6.5 0 0 1 0-13zM15.3 15.3L20 20",
            pin: "M9 4h6M10 4v5.5L7 13v1.5h10V13l-3-3.5V4M12 14.5V20"
        })

    implicitWidth: Tokens.iconSize
    implicitHeight: implicitWidth

    Shape {
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            strokeColor: icon.color
            strokeWidth: Tokens.iconStroke * icon.width / 24
            fillColor: icon.filled ? icon.color : "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            scale: Qt.size(icon.width / 24, icon.height / 24)
            PathSvg {
                path: icon.paths[icon.name] ?? ""
            }
        }
    }
}
