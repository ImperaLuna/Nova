import QtQuick
import "../bar"
import "../theme"
import "../theme/icons.js" as Icons

// Quick settings' button on the right pill, picked while its panel is open
// on this screen. It carries a badge: how many notifications are unread,
// or a grey crossed-out bell during do not disturb; none while it's open.
PillButton {
    id: button

    property bool open: false
    property int unread: 0
    property bool doNotDisturb: false

    icon: Icons.TUNE
    picked: open

    Rectangle {
        id: badge
        objectName: "badge"
        readonly property bool shown: !button.open && (button.doNotDisturb || button.unread > 0)
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.rightMargin: -Tokens.badgeOffset
        anchors.topMargin: -Tokens.badgeOffset
        width: Math.max(height, count.implicitWidth + 2 * Tokens.badgePad)
        height: Tokens.badgeSize
        radius: height / 2
        color: button.doNotDisturb ? Tokens.badgeQuiet : Tokens.primary
        border.width: Tokens.badgeRing
        border.color: Tokens.bg
        scale: shown ? 1 : Tokens.enterScale
        opacity: shown ? 1 : 0

        Behavior on scale {
            NumberAnimation {
                duration: Tokens.durationColor
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }
        Behavior on opacity {
            NumberAnimation {
                duration: Tokens.durationFast
            }
        }

        Text {
            id: count
            anchors.centerIn: parent
            visible: !button.doNotDisturb
            font.family: Tokens.fontMono
            font.pixelSize: Tokens.fontSizeMicro
            font.weight: Tokens.weightBadge
            color: Tokens.onAccent
            text: button.unread > Tokens.badgeMax ? `${Tokens.badgeMax}+` : button.unread
        }
        LineIcon {
            anchors.centerIn: parent
            visible: button.doNotDisturb
            path: Icons.BELL_OFF
            size: Tokens.badgeIcon
            color: Tokens.onAccent
        }
    }
}
