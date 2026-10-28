import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// A device's little battery: a rim with a nub, filled to `fill` (0 to 1),
// in the error color while low and the success color with a bolt while
// charging. A new level eases in.
Item {
    id: battery

    property real fill: 0
    property bool low: false
    property bool charging: false
    readonly property color tone: charging ? Tokens.success : low ? Tokens.error : Tokens.fg

    implicitWidth: Tokens.deviceBatteryWidth + Tokens.deviceBatteryNubWidth
    implicitHeight: Tokens.deviceBatteryHeight

    Rectangle {
        id: body
        width: Tokens.deviceBatteryWidth
        height: parent.height
        radius: Tokens.deviceBatteryRadius
        color: "transparent"
        border.width: Tokens.deviceBatteryRim
        border.color: battery.low && !battery.charging ? Tokens.error : Tokens.alpha(Tokens.fg, Tokens.deviceBatteryRimOpacity)

        Rectangle {
            objectName: "level"
            x: Tokens.deviceBatteryInset
            y: Tokens.deviceBatteryInset
            height: parent.height - 2 * Tokens.deviceBatteryInset
            width: (parent.width - 2 * Tokens.deviceBatteryInset) * battery.fill
            radius: Tokens.deviceBatteryRadius - Tokens.deviceBatteryInset / 2
            color: battery.tone
            Behavior on width {
                NumberAnimation {
                    duration: Tokens.durationFill
                    easing.type: Easing.OutCubic
                }
            }
        }

        LineIcon {
            anchors.centerIn: parent
            visible: battery.charging
            path: Icons.BOLT
            fill: Icons.BOLT
            size: Tokens.deviceBoltSize
            color: Tokens.onAccent
        }
    }

    Rectangle {
        anchors.left: body.right
        anchors.verticalCenter: body.verticalCenter
        width: Tokens.deviceBatteryNubWidth
        height: Tokens.deviceBatteryNubHeight
        radius: width / 2
        color: body.border.color
    }
}
