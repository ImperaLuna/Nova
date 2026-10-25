pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import "../bar"
import "../demo"
import "../launcher"
import "../launcher/service"
import "../notifications/service"
import "../power"
import "../power/service"
import "../quick_settings"
import "../quick_settings/service"
import "../daemon"
import "../theme"
import "../daemon/protocol.js" as Protocol

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

    // Null once the output is gone: everything here lays out by `output`.
    required property ShellScreen screen
    readonly property StageScreen output: StageScreen {
        screen: root.screen
    }
    // Its panels just stopped and their final state (and regions) isn't in
    // a frame yet, or a pill just showed again: drawing stays on for it even
    // while another surface moves.
    property bool settling: false

    // Every panel on this screen, whatever surface it's drawn on.
    readonly property list<StagePanel> panels: [...bar.panels, ...(power.item ? [power.item] : []), ...(quickSettings.item ? [quickSettings.item] : []), ...(launcher.item ? [launcher.item] : []), ...(demo.item?.panels ?? [])]
    // While a panel is open or closing on this screen, everything Nova draws
    // here sits on the Overlay layer (PanelSurface always does): niri and Hyprland draw a focused
    // fullscreen window over the Top layer, which hid the launcher under it.
    // Otherwise it's Top, so the bar stays below fullscreen video and games.
    readonly property int surfaceLayer: all.shown.length > 0 ? WlrLayer.Overlay : WlrLayer.Top
    // A new layer goes out with a frame, and the stage may not be drawing
    // (another surface's panel opened).
    onSurfaceLayerChanged: settling = true
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
        panels: root.placed("top")
        fixed: bar.fixedShapes
    }

    // Something on the stage animates: a panel, or the bar sliding aside.
    readonly property bool moving: drawn.moving || bar.sliding
    readonly property string motionName: `${root.output.name}/top`
    onMovingChanged: {
        StageMotion.setMoving(motionName, moving);
        if (!moving)
            settling = true;
    }
    Component.onDestruction: StageMotion.setMoving(motionName, false)

    // The power menu on the right edge, while its feature is on.
    LazyLoader {
        id: power
        active: PowerService.ready
        component: PowerMenu {
            output: root.output
        }
    }

    // The quick settings sidebar, while its feature is on.
    LazyLoader {
        id: quickSettings
        active: QuickSettingsService.ready
        component: QuickSettings {
            output: root.output
            pill: bar.rightPillBox
            pillItem: bar.rightPillItem
            power: PowerService.ready
            onQuickSettingsClicked: all.toggle(Protocol.FEATURE_QUICK_SETTINGS, root.output.name, root.output.name)
            // The power menu takes its place.
            onPowerClicked: {
                all.toggle(Protocol.FEATURE_QUICK_SETTINGS, root.output.name, root.output.name);
                all.toggle(Protocol.FEATURE_POWER, root.output.name, root.output.name);
            }
        }
    }

    // The launcher in the middle, while its feature is on.
    LazyLoader {
        id: launcher
        active: LauncherService.ready
        component: Launcher {
            id: deck
            screenWidth: root.output.width
            screenHeight: root.output.height
            apps: LauncherService.current.apps
            pinned: LauncherService.current.pinned
            recent: LauncherService.current.recent
            iconSource: icon => LauncherService.iconSource(icon)
            onSearchRequested: query => LauncherService.search(query, found => deck.showMatches(query, found))
            onLaunchRequested: (id, newWindow) => LauncherService.launch(id, root.output.name, newWindow, error => deck.launched(error))
            onPinRequested: (id, pinned) => LauncherService.pin(id, pinned)
        }
    }

    // Placeholder panels for each placement, with NOVA_DEMO_PANELS=1 only
    // (ui/demo).
    LazyLoader {
        id: demo
        active: Quickshell.env("NOVA_DEMO_PANELS") === "1"
        component: DemoPanels {
            output: root.output
            centralPill: bar.centralPill
        }
    }

    // `nova toggle <panel>`, which novad sends every screen. When novad
    // doesn't know which output is focused, the first screen takes it.
    Connections {
        target: Daemon
        function onToggleRequested(panel: string, output: var) {
            all.toggle(panel, root.output.name, output ?? Quickshell.screens[0]?.name ?? "");
        }
    }

    PanelSurface {
        id: right
        output: root.output
        placement: "right"
        panels: root.placed("right")
        clickAnywhere: all.catchesClicks
        holding: root.settling
        onDismissRequested: all.dismiss()
    }

    PanelSurface {
        id: center
        output: root.output
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
        updatesEnabled: root.moving || root.settling || StageMotion.surfaces.length === 0

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

        WlrLayershell.layer: root.surfaceLayer
        WlrLayershell.namespace: "nova-stage"
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
            width: root.output.width
            height: root.output.height
            enabled: all.catchesClicks
            focus: true
            onClicked: mouse => {
                if (!drawn.covers(mouse.x, mouse.y))
                    all.dismiss();
            }
            // A scroll outside closes it too, so the next one reaches the
            // window below: the screen's input can't pass it on.
            onWheel: wheel => {
                if (!drawn.covers(wheel.x, wheel.y))
                    all.dismiss();
            }
            Keys.onEscapePressed: all.dismiss()
        }

        Bar {
            id: bar
            anchors.fill: parent
            screenName: root.output.name
            // A panel of the bar's is open: on the stage, or grown out of
            // one of its pills. Not `moving`, which a closed pill's own
            // animations (a loading spinner) set too.
            held: root.panels.some(panel => panel.open && (panel.placement === "top" || panel.from !== null))
            // Its items that a panel elsewhere is drawn over (StagePanel
            // `from`), so they hide. They show again once that panel has
            // closed, and the surface it's on stays until that's drawn.
            covered: root.panels.filter(panel => panel.covering && panel.from).map(panel => panel.from)
            onCoveredChanged: root.settling = true
            quickSettings: QuickSettingsService.ready
            power: PowerService.ready
            quickSettingsOpen: quickSettings.item?.open ?? false
            unread: NotificationsService.ready ? NotificationsService.unread : 0
            doNotDisturb: NotificationsService.ready && NotificationsService.doNotDisturb
            onQuickSettingsClicked: all.toggle(Protocol.FEATURE_QUICK_SETTINGS, root.output.name, root.output.name)
            onPowerClicked: all.toggle(Protocol.FEATURE_POWER, root.output.name, root.output.name)
            onLauncherRequested: {
                const deck = launcher.item as Launcher;
                if (deck)
                    deck.open = !deck.open;
            }
        }

        // Keep awake (quick settings): the compositor doesn't idle while a
        // shown surface holds an inhibitor, and the stage always shows.
        IdleInhibitor {
            window: stage
            enabled: QuickSettingsService.keepAwake
        }

        FrameLog {
            surfaceName: root.output.name
        }
    }

    ClickCatcher {
        screen: root.screen
        catching: all.catchesClicks
        surfaceLayer: root.surfaceLayer
        // Only while catching, so it doesn't redraw for every morph frame.
        holes: all.catchesClicks ? [Qt.rect(0, 0, root.output.width, stage.implicitHeight), ...[right, center].filter(surface => surface.visible).map(surface => surface.box)] : []
        onClicked: all.dismiss()
    }
}
