import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// The right pill: the quick settings button and the orange power button,
// each shown while its feature is on. Glass, like the other pills; a fixed
// shape on the stage, since its panels open on the right edge's surface.
Item {
    id: pill

    property bool quickSettings: false
    property bool power: false
    // Its panel is open on this screen: the button shows it's picked.
    property bool quickSettingsOpen: false

    signal quickSettingsClicked
    signal powerClicked

    readonly property bool glass: true
    readonly property real radius: height / 2

    implicitWidth: buttons.implicitWidth + 2 * Tokens.pillButtonInset
    implicitHeight: Tokens.barHeight

    GlassShadow {
        glass: surface
    }

    Glass {
        id: surface
        anchors.fill: parent
        radius: pill.radius
    }

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

    Row {
        id: buttons
        anchors.centerIn: parent
        spacing: Tokens.pillButtonGap

        PillButton {
            visible: pill.quickSettings
            icon: Icons.TUNE
            picked: pill.quickSettingsOpen
            onClicked: pill.quickSettingsClicked()
        }
        PillButton {
            visible: pill.power
            icon: Icons.POWER
            ink: Tokens.primary
            onClicked: pill.powerClicked()
        }
    }
}
