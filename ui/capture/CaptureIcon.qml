import QtQuick
import QtQuick.Shapes
import "../theme"
import "../daemon/protocol.js" as Protocol
import "capture.js" as Capture

// The capture toolbar's and the screenshot card's line icons, stroked in
// `color`: a Protocol.CAPTURE_* mode, a Protocol.SHOT_* action, or a
// Capture.ICON_* name. 24x24 outlines like PowerIcon's.
Item {
    id: icon

    required property string name
    property color color: Tokens.fg

    readonly property var paths: ({
            [Protocol.CAPTURE_REGION]: "M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3",
            [Protocol.CAPTURE_SCREEN]: "M5 4h14a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zM8 21h8M12 17v4",
            [Protocol.SHOT_OPEN]: "M14 4h6v6M20 4l-9 9M18 14v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4",
            [Protocol.SHOT_FOLDER]: "M4 7a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z",
            [Protocol.SHOT_DELETE]: "M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3",
            [Capture.ICON_CLOSE]: "M6 6l12 12M18 6L6 18",
            [Capture.ICON_CHECK]: "M5 12.5l4.5 4.5L19 7.5"
        })

    implicitWidth: Tokens.iconSize
    implicitHeight: implicitWidth

    Shape {
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            strokeColor: icon.color
            strokeWidth: Tokens.iconStroke * icon.width / 24
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            scale: Qt.size(icon.width / 24, icon.height / 24)
            PathSvg {
                path: icon.paths[icon.name] ?? ""
            }
        }
    }
}
