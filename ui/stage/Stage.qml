pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import "../bar"
import "../demo"
import "../daemon"
import "../theme"

// One per screen: a transparent layer across the top of the screen that the
// bar, its panels and popups are all drawn on, so a pill can grow into its
// panel. Panels elsewhere on the screen (StagePanel `placement`) are drawn
// on a PanelSurface per placement instead. It's as tall as the tallest of them open (plus shadow), not the
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
    // Its panels just stopped and their final state (and regions) isn't in
    // a frame yet, or a pill just showed again: drawing stays on for it even
    // while another surface moves.
    property bool settling: false

    // Every panel on this screen, whatever surface it's drawn on.
    readonly property list<StagePanel> panels: [...bar.panels, ...(demo.item?.panels ?? [])]
    function placed(placement: string): list<StagePanel> {
        return panels.filter(panel => panel.placement === placement);
    }

    // The whole screen's: which panel holds the keyboard, toggling, closing.
    StagePanels {
        id: all
        panels: root.panels
    }

    // What's on the stage itself.
    StagePanels {
        id: drawn
        readonly property string motionName: `${root.screen.name}/top`
        panels: root.placed("top")
        fixed: bar.fixedShapes
        onMovingChanged: {
            StageMotion.setMoving(motionName, moving);
            if (!moving)
                root.settling = true;
        }
        Component.onDestruction: StageMotion.setMoving(motionName, false)
    }

    // Placeholder panels for each placement, with NOVA_DEMO_PANELS=1 only
    // (ui/demo).
    LazyLoader {
        id: demo
        active: Quickshell.env("NOVA_DEMO_PANELS") === "1"
        component: DemoPanels {
            screen: root.screen
            centralPill: bar.centralPill
        }
    }

    // `nova toggle <panel>`, which novad sends every screen. When novad
    // doesn't know which output is focused, the first screen takes it.
    Connections {
        target: Daemon
        function onToggleRequested(panel: string, output: var) {
            all.toggle(panel, root.screen.name, output ?? Quickshell.screens[0]?.name ?? "");
        }
    }

    PanelSurface {
        id: right
        output: root.screen
        placement: "right"
        panels: root.placed("right")
        clickAnywhere: all.catchesClicks
        holding: root.settling
        onDismissRequested: all.dismiss()
    }

    PanelSurface {
        id: center
        output: root.screen
        placement: "center"
        panels: root.placed("center")
        clickAnywhere: all.catchesClicks
        holding: root.settling
        onDismissRequested: all.dismiss()
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
        // Draws while something moves here, or while nothing moves anywhere
        // (StageMotion). Its regions only change when its own panels move,
        // and it draws until their last frame is out, so they're always
        // sent: Quickshell sends them only with a frame.
        updatesEnabled: drawn.moving || root.settling || StageMotion.surfaces.length === 0

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
        // windows below. While a panel that closes on a click outside is
        // open, here or on another surface, the whole stage takes clicks
        // (and ClickCatcher the rest of the screen), and one outside the
        // panel closes it.
        mask: Region {
            regions: inputRegions.instances
        }

        Variants {
            id: inputRegions
            model: all.catchesClicks ? [outside] : drawn.shapes
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

        // The whole screen: Hyprland gives a surface that holds the keyboard
        // every click on the screen (see PanelSurface's `outside`).
        MouseArea {
            id: outside
            readonly property int radius: 0
            width: root.screen.width
            height: root.screen.height
            enabled: all.catchesClicks
            focus: true
            onClicked: mouse => {
                if (!drawn.covers(mouse.x, mouse.y))
                    all.dismiss();
            }
            Keys.onEscapePressed: all.dismiss()
        }

        Bar {
            id: bar
            anchors.fill: parent
            screenName: root.screen.name
            // Its items that a panel elsewhere is drawn over (StagePanel
            // `from`), so they hide. They show again once that panel has
            // closed, and the surface it's on stays until that's drawn.
            covered: root.panels.filter(panel => panel.covering && panel.from).map(panel => panel.from)
            onCoveredChanged: root.settling = true
        }

        FrameLog {
            surfaceName: root.screen.name
        }
    }

    ClickCatcher {
        screen: root.screen
        catching: all.catchesClicks
        // Only while catching, so it doesn't redraw for every morph frame.
        holes: all.catchesClicks ? [Qt.rect(0, 0, root.screen.width, stage.implicitHeight), ...[right, center].filter(surface => surface.visible).map(surface => surface.box)] : []
        onClicked: all.dismiss()
    }
}
