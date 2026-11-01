pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import "../bar"
import "../compositor"
import "../demo"
import "../launcher"
import "../launcher/service"
import "../notifications/service"
import "../power"
import "../power/service"
import "../quick_settings"
import "../quick_settings/service"
import "../wallpaper"
import "../wallpaper/service"
import "../daemon"
import "../theme"
import "../daemon/protocol.js" as Protocol
import "../wallpaper/carousel.js" as Carousel
import "../icons"

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
    // Its panels just stopped and their final state (and regions) isn't on
    // screen yet, or a pill just showed again: drawing stays on for it even
    // while another surface moves.
    readonly property bool settling: drawing.settling
    // A panel that closes on a click outside is open on this screen.
    readonly property bool catchesClicks: all.catchesClicks
    // One is open on any screen: a click anywhere off the panels, on this
    // screen too, closes it. The shell sets it from every screen's
    // `catchesClicks`.
    property bool catchesAnywhere: catchesClicks
    // The stage's mask changes with it, and goes out with a frame.
    onCatchesAnywhereChanged: drawing.settle()

    // Every panel on this screen, whatever surface it's drawn on.
    readonly property list<StagePanel> panels: [...bar.panels, ...(power.item ? [power.item] : []), ...(quickSettings.item ? [quickSettings.item] : []), ...(launcher.item ? [launcher.item] : []), ...(wallpaper.item ? [wallpaper.item] : []), ...(demo.item?.panels ?? [])]
    // While a panel is open or closing on this screen, everything Nova draws
    // here sits on the Overlay layer (PanelSurface always does): niri and Hyprland draw a focused
    // fullscreen window over the Top layer, which hid the launcher under it.
    // Otherwise it's Top.
    readonly property int panelLayer: all.shown.length > 0 ? WlrLayer.Overlay : WlrLayer.Top
    // The stage's layer: also Overlay while the bar steps aside for a
    // fullscreen window, so touching the top edge brings the pills back over
    // it. Hyprland gives a Top layer under a focused fullscreen window no
    // pixels and no pointer, unless it mapped while that window was already
    // fullscreen, so peeking worked only now and then (IMP-182). The pills
    // are out of sight until the pointer gets there, so video and games
    // still show nothing of the bar.
    readonly property int surfaceLayer: panelLayer === WlrLayer.Overlay || bar.fullscreenBelow ? WlrLayer.Overlay : WlrLayer.Top
    // A new layer goes out with a frame, and the stage may not be drawing
    // (another surface's panel opened).
    onSurfaceLayerChanged: drawing.settle()
    // Nothing of Nova's but the bar is on this screen: a Backdrop can copy
    // it.
    readonly property bool quiet: all.shown.length === 0 && !NotificationsService.popUpsShown
    // Where the PanelSurfaces with panels shown are, in screen coordinates.
    readonly property list<rect> panelBoxes: [right, center].filter(surface => !surface.parked).map(surface => surface.box)
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

    // When the stage draws: while something on it animates (a panel, or
    // the bar sliding aside), or nothing moves anywhere.
    SurfaceDrawing {
        id: drawing
        window: stage
        name: `${root.output.name}/top`
        moving: drawn.moving || bar.sliding
    }

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
            iconSource: icon => AppIcons.source(icon)
            onSearchRequested: query => LauncherService.search(query, found => deck.showMatches(query, found))
            onLaunchRequested: (id, newWindow) => LauncherService.launch(id, root.output.name, newWindow, error => deck.launched(error))
            onPinRequested: (id, pinned) => LauncherService.pin(id, pinned)
        }
    }

    // The wallpaper carousel over the whole screen, while its feature is on.
    LazyLoader {
        id: wallpaper
        active: WallpaperService.ready
        component: WallpaperCarousel {
            id: carousel
            screenWidth: root.output.width
            screenHeight: root.output.height
            outputName: root.output.name
            screens: Carousel.screenOrder(Quickshell.screens)
            compositorFocus: Protocol.focusKey(Compositor.focusedWorkspace)
            onLoadRequested: opening => WallpaperService.load((error, listing, shown) => carousel.loaded(opening, error, listing, shown))
            onChosen: requests => WallpaperService.setAll(requests, error => carousel.applied(error))
        }
    }

    // What the carousel previews while it's open or closing here, for the
    // other screens. Opening it here closes it elsewhere, which may still be
    // fading: an open one's preview wins, and a closing one only changes or
    // clears its own.
    readonly property var wallpaperPreview: {
        const carousel = wallpaper.item as WallpaperCarousel;
        if (!carousel || !carousel.shown)
            return null;
        return {
            paths: carousel.previews,
            choosingFor: carousel.choosingFor,
            output: root.output.name,
            shown: carousel.open
        };
    }
    onWallpaperPreviewChanged: {
        const ours = WallpaperService.preview?.output === root.output.name;
        if (wallpaperPreview && (wallpaperPreview.shown || ours || !WallpaperService.preview))
            WallpaperService.preview = wallpaperPreview;
        else if (!wallpaperPreview && ours)
            WallpaperService.preview = null;
    }

    // Another screen's carousel previews this screen's pick here, and
    // marks it while it picks for it.
    LazyLoader {
        readonly property var preview: WallpaperService.preview
        active: preview !== null && preview.output !== root.output.name
        component: WallpaperPreview {
            screen: root.screen
            path: WallpaperService.preview?.paths[root.output.name] ?? ""
            shown: WallpaperService.preview?.shown ?? false
            picking: WallpaperService.preview?.choosingFor === root.output.name
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

    // Esc, or a click off the panels, here or on another screen.
    Connections {
        target: OpenPanels
        function onDismissed() {
            all.dismiss();
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
        quiet: root.quiet
        onDismissRequested: OpenPanels.dismissed()
    }

    PanelSurface {
        id: center
        output: root.output
        placement: "center"
        panels: root.placed("center")
        clickAnywhere: all.catchesClicks
        holding: root.settling
        quiet: root.quiet
        onDismissRequested: OpenPanels.dismissed()
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
        updatesEnabled: drawing.draws

        WlrLayershell.layer: root.surfaceLayer
        WlrLayershell.namespace: "nova-stage"
        WlrLayershell.keyboardFocus: drawn.needsKeyboard ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

        // Rounded like what's drawn, so clicks at the corners reach the
        // windows below. While a panel that closes on a click outside is
        // open, on any surface or screen, the whole stage takes clicks
        // (and ClickCatcher the rest of the screen), and one outside the
        // panel closes it. Both regions follow the bar's pills as they
        // slide with their shelf; without that they kept taking clicks
        // where they had slid out of sight (IMP-118).
        mask: regions.mask
        StageRegions {
            id: regions
            window: stage
            inputShapes: root.catchesAnywhere ? [outside] : drawn.shapes
            glassShapes: drawn.glassShapes
            offset: bar.slideOffset
            // The PanelSurfaces stay mapped (parked) and can sit under the
            // stage, which moves to Overlay when a panel opens: they take
            // the clicks over themselves, as with ClickCatcher (IMP-148).
            holes: root.catchesAnywhere ? root.panelBoxes : []
        }

        DismissArea {
            id: outside
            output: root.output
            enabled: root.catchesAnywhere
            stagePanels: drawn
            onDismissRequested: OpenPanels.dismissed()
        }

        // For its glass to thicken over bright windows. A copy of the screen
        // sees only what's behind while no panel or pop-up is out.
        Backdrop {
            screen: root.screen
            quiet: root.quiet
        }

        Bar {
            id: bar
            anchors.fill: parent
            screenName: root.output.name
            screenHeight: root.output.height
            // A panel of the bar's is open: on the stage, or grown out of
            // one of its pills. Not `moving`, which a closed pill's own
            // animations (a loading spinner) set too.
            held: root.panels.some(panel => panel.open && (panel.placement === "top" || panel.from !== null))
            // Its items that a panel elsewhere is drawn over (StagePanel
            // `from`), so they hide. They show again once that panel has
            // closed, and the surface it's on stays until that's drawn.
            covered: root.panels.filter(panel => panel.covering && panel.from).map(panel => panel.from)
            onCoveredChanged: drawing.settle()
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
        catching: root.catchesAnywhere
        // Over a fullscreen window while catching, also for a panel open on
        // another screen.
        surfaceLayer: root.catchesAnywhere ? WlrLayer.Overlay : root.panelLayer
        // Only while catching, so it doesn't redraw for every morph frame.
        holes: root.catchesAnywhere ? [Qt.rect(0, 0, root.output.width, stage.implicitHeight), ...root.panelBoxes] : []
        onClicked: OpenPanels.dismissed()
    }
}
