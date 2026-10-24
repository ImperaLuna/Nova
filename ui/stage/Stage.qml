pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import "../bar"
import "../daemon"
import "../theme"

// One per screen: a transparent layer across the top of the screen that the
// bar, its panels and popups are all drawn on, so a pill can grow into its
// panel. It's as tall as the tallest of them open (plus shadow), not the
// whole screen: niri blurs the whole surface behind glass on every frame, so
// a full-screen stage cost a full-screen blur per frame of every morph
// (IMP-54). It reserves no space (ExclusionZone keeps the bar's) and takes
// input only where something is drawn; clicks anywhere else reach the
// windows below.
// The bar lists what it draws as `panels` (StagePanel) and `fixedShapes`;
// everything below follows from those.
Scope {
    id: root

    required property ShellScreen screen

    StagePanels {
        id: drawn
        panels: bar.panels
        fixed: bar.fixedShapes
    }

    // `nova toggle <panel>`, which novad sends every screen. When novad
    // doesn't know which output is focused, the first screen takes it.
    Connections {
        target: Daemon
        function onToggleRequested(panel: string, output: var) {
            drawn.toggle(panel, root.screen.name, output ?? Quickshell.screens[0]?.name ?? "");
        }
    }

    ExclusionZone {
        screen: root.screen
        size: bar.reservedHeight
    }

    PanelWindow { // qmllint disable uncreatable-type
        id: stage

        screen: root.screen
        anchors {
            top: true
            left: true
            right: true
        }
        implicitHeight: Math.ceil(drawn.reach + Tokens.shadowRoom)
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        WlrLayershell.layer: WlrLayer.Top
        WlrLayershell.keyboardFocus: drawn.needsKeyboard ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

        // Rounded like what's drawn, so clicks at the corners reach the
        // windows below. While a panel holds the keyboard, the whole stage
        // takes clicks (and `below` the rest of the screen), and one outside
        // the panel closes it.
        mask: Region {
            regions: inputRegions.instances
        }

        Variants {
            id: inputRegions
            model: drawn.needsKeyboard ? [outside] : drawn.shapes
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
            model: drawn.glassShapes
            ShapeRegion {}
        }

        // Asked for only once there's glass: asking at all logs a warning on
        // compositors without blur, which a screen with no glass shouldn't do.
        readonly property bool hasGlass: drawn.glassShapes.length > 0
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
            enabled: drawn.needsKeyboard
            focus: true
            onClicked: drawn.dismiss()
            Keys.onEscapePressed: drawn.dismiss()
        }

        Bar {
            id: bar
            anchors.fill: parent
        }
    }

    // The screen below the stage, there only while a panel holds the
    // keyboard, so a click anywhere off the panel closes it. Empty, and
    // never blurred.
    PanelWindow { // qmllint disable uncreatable-type
        id: below

        screen: root.screen
        visible: drawn.needsKeyboard
        anchors {
            bottom: true
            left: true
            right: true
        }
        implicitHeight: root.screen.height - stage.implicitHeight
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        WlrLayershell.layer: WlrLayer.Top

        MouseArea {
            anchors.fill: parent
            onClicked: drawn.dismiss()
        }
    }
}
