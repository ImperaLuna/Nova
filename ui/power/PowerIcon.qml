import QtQuick
import QtQuick.Shapes
import "../theme"
import "../daemon/protocol.js" as Protocol

// A power menu action's line icon, stroked in `color`. 24x24 outlines in the
// style of the design mockups.
Item {
    id: icon

    required property string action
    property color color: Tokens.fg

    readonly property var paths: ({
            [Protocol.POWER_LOCK]: "M7.5 11h9a2.5 2.5 0 0 1 2.5 2.5v5a2.5 2.5 0 0 1-2.5 2.5h-9a2.5 2.5 0 0 1-2.5-2.5v-5a2.5 2.5 0 0 1 2.5-2.5zM8 11V8a4 4 0 0 1 8 0v3",
            [Protocol.POWER_SUSPEND]: "M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z",
            [Protocol.POWER_LOG_OUT]: "M14 4h4a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-4M10 8l-4 4 4 4M6 12h10",
            [Protocol.POWER_REBOOT]: "M4 12a8 8 0 1 0 2.4-5.7M4 4v4h4",
            [Protocol.POWER_POWER_OFF]: "M12 3v8M6.3 7.2a8 8 0 1 0 11.4 0"
        })

    implicitWidth: Tokens.iconSizeMenu
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
                path: icon.paths[icon.action] ?? ""
            }
        }
    }
}
