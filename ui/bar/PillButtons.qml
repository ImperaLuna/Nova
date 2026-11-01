import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// The right pill's buttons: quick settings and the orange power button,
// each shown while its feature is on. On the pill, and in the same place
// on quick settings, which grows out of the pill over it. The quick
// settings button carries a badge: how many notifications are unread, or
// a grey crossed-out bell during do not disturb; none while it's open.
Row {
    id: buttons

    property bool quickSettings: false
    property bool power: false
    // Quick settings is open on this screen: its button shows it's picked.
    property bool quickSettingsOpen: false
    property int unread: 0
    property bool doNotDisturb: false

    signal quickSettingsClicked
    signal powerClicked

    spacing: Tokens.pillButtonGap

    component PillButton: Rectangle {
        id: button
        required property string icon
        property color ink: Tokens.fg
        property bool picked: false
        signal clicked

        width: Tokens.pillButtonSize
        height: Tokens.pillButtonSize
        radius: width / 2
        color: picked ? Tokens.selectedQuiet : press.containsMouse ? Tokens.hover : "transparent"

        Behavior on color {
            ColorAnimation {
                duration: Tokens.durationFast
            }
        }

        LineIcon {
            anchors.centerIn: parent
            path: button.icon
            color: button.ink
        }

        MouseArea {
            id: press
            anchors.fill: parent
            hoverEnabled: true
            onClicked: button.clicked()
        }
    }

    PillButton {
        visible: buttons.quickSettings
        icon: Icons.TUNE
        picked: buttons.quickSettingsOpen
        onClicked: buttons.quickSettingsClicked()

        Rectangle {
            id: badge
            objectName: "badge"
            readonly property bool shown: !buttons.quickSettingsOpen && (buttons.doNotDisturb || buttons.unread > 0)
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.rightMargin: -Tokens.badgeOffset
            anchors.topMargin: -Tokens.badgeOffset
            width: Math.max(height, count.implicitWidth + 2 * Tokens.badgePad)
            height: Tokens.badgeSize
            radius: height / 2
            color: buttons.doNotDisturb ? Tokens.badgeQuiet : Tokens.primary
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
                visible: !buttons.doNotDisturb
                font.family: Tokens.fontMono
                font.pixelSize: Tokens.fontSizeMicro
                font.weight: Tokens.weightBadge
                color: Tokens.onAccent
                text: buttons.unread > Tokens.badgeMax ? `${Tokens.badgeMax}+` : buttons.unread
            }
            LineIcon {
                anchors.centerIn: parent
                visible: buttons.doNotDisturb
                path: Icons.BELL_OFF
                size: Tokens.badgeIcon
                color: Tokens.onAccent
            }
        }
    }
    PillButton {
        visible: buttons.power
        icon: Icons.POWER
        ink: Tokens.primary
        onClicked: buttons.powerClicked()
    }
}
