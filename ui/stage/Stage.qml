pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import "../bar"
import "../daemon"

// One per screen: a transparent layer over the whole screen that the bar,
// its panels and popups are all drawn on, so a pill can grow into its panel.
// It reserves no space (ExclusionZone keeps the bar's) and takes input only
// where something is drawn; clicks anywhere else reach the windows below.
// The bar lists what it draws as `panels` (StagePanel) and `fixedShapes`;
// everything below follows from those.
Scope {
    id: root

    required property ShellScreen screen
    // Its panels just stopped and their final state (and regions) isn't in
    // a frame yet: drawing stays on for it even while another screen moves.
    property bool settling: false

    StagePanels {
        id: drawn
        panels: bar.panels
        fixed: bar.fixedShapes
        onMovingChanged: {
            StageMotion.setMoving(root.screen.name, moving);
            if (!moving)
                root.settling = true;
        }
        Component.onDestruction: StageMotion.setMoving(root.screen.name, false)
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
            bottom: true
            left: true
            right: true
        }
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        // Draws while something moves here, or while nothing moves anywhere
        // (StageMotion). Its regions only change when its own panels move,
        // and it draws until their last frame is out, so they're always
        // sent: Quickshell sends them only with a frame.
        updatesEnabled: drawn.moving || root.settling || StageMotion.screens.length === 0

        // Qt emits afterAnimating once it has polished a frame (Quickshell
        // sends the regions then) and right before syncing it, and that frame
        // goes out even if updates go off now. A later frameSwapped could
        // still be an older frame's.
        Connections {
            target: stage.contentItem.Window.window
            enabled: root.settling
            function onAfterAnimating() {
                root.settling = false;
            }
        }

        WlrLayershell.layer: WlrLayer.Top
        WlrLayershell.keyboardFocus: drawn.needsKeyboard ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

        // Rounded like what's drawn, so clicks at the corners reach the
        // windows below. While a panel holds the keyboard, the whole screen
        // takes clicks, and one outside the panel closes it.
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

        FrameLog {
            screenName: root.screen.name
        }
    }
}
