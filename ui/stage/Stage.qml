pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import "../bar"

// One per screen: a transparent layer over the whole screen that the bar,
// its panels and popups are all drawn on, so a pill can grow into its panel.
// It reserves no space (ExclusionZone keeps the bar's) and takes input only
// where something is drawn; clicks anywhere else reach the windows below.
Scope {
    id: root

    required property ShellScreen screen

    ExclusionZone {
        screen: root.screen
        size: bar.reservedHeight
    }

    PanelWindow { // qmllint disable uncreatable-type
        id: stage

        screen: root.screen
        anchors {
            top: true
            bottom: true
            left: true
            right: true
        }
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        WlrLayershell.layer: WlrLayer.Top
        WlrLayershell.keyboardFocus: bar.needsKeyboard ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

        // Rounded like what's drawn, so clicks at the corners reach the
        // windows below. While a panel holds the keyboard, the whole screen
        // takes clicks, and one outside the panel closes it.
        mask: Region {
            regions: inputRegions.instances
        }

        Variants {
            id: inputRegions
            model: bar.needsKeyboard ? [outside] : bar.shapes
            ShapeRegion {}
        }

        // Real blur behind the glass where the compositor offers it
        // (ext-background-effect-v1: niri 26.04+, Hyprland 0.56+); elsewhere
        // the tint stands alone.
        Region {
            id: blur
            regions: blurRegions.instances
        }

        Variants {
            id: blurRegions
            model: bar.glassShapes
            ShapeRegion {}
        }

        // Asked for only once there's glass: asking at all logs a warning on
        // compositors without blur, which a screen with no glass shouldn't do.
        readonly property bool hasGlass: bar.glassShapes.length > 0
        function askForBlur() {
            if (hasGlass)
                BackgroundEffect.blurRegion = blur;
        }
        onHasGlassChanged: askForBlur()
        Component.onCompleted: askForBlur()

        MouseArea {
            id: outside
            readonly property int radius: 0
            anchors.fill: parent
            enabled: bar.needsKeyboard
            focus: true
            onClicked: bar.dismiss()
            Keys.onEscapePressed: bar.dismiss()
        }

        Bar {
            id: bar
            anchors.fill: parent
            screenName: root.screen.name
        }
    }
}
