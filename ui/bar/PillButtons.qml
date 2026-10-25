import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// The right pill's buttons: quick settings and the orange power button,
// each shown while its feature is on. On the pill, and in the same place
// on quick settings, which grows out of the pill over it.
Row {
    id: buttons

    property bool quickSettings: false
    property bool power: false
    // Quick settings is open on this screen: its button shows it's picked.
    property bool quickSettingsOpen: false

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
    }
    PillButton {
        visible: buttons.power
        icon: Icons.POWER
        ink: Tokens.primary
        onClicked: buttons.powerClicked()
    }
}
