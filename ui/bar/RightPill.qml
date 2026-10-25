import QtQuick
import "../theme"

// The right pill: the quick settings button and the orange power button
// (PillButtons), each shown while its feature is on. Glass, like the other
// pills; a fixed shape on the stage, since its panels open on the right
// edge's surface. Quick settings grows out of it and hides it meanwhile.
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

    PillButtons {
        id: buttons
        anchors.centerIn: parent
        quickSettings: pill.quickSettings
        power: pill.power
        quickSettingsOpen: pill.quickSettingsOpen
        onQuickSettingsClicked: pill.quickSettingsClicked()
        onPowerClicked: pill.powerClicked()
    }
}
