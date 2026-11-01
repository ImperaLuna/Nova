pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import "../compositor"
import "../theme"
import "../daemon/protocol.js" as Protocol

// A small copy of the screen behind a window's glass, for its GlassTint to
// thicken over bright spots (GlassBackdrops). Put one in each window whose
// glass should adapt; a texture only draws in the window it was made in.
//
// The copy is Nova's too, so it's taken only while nothing of Nova's but
// the bar is on the screen (`quiet`): glass that sees itself would thin out,
// then thicken again on the next copy. The bar's row is never looked at
// (`skipTop`). A copy is taken once it can be, then a moment after the
// focused window, its title or the focused workspace last changed (a page
// that changes, mostly changes the title), or once it's quiet again after
// that. It's downscaled on the GPU into a few texels and never shown.
Item {
    id: backdrop

    required property ShellScreen screen
    // The window's top-left on the screen.
    property point origin: Qt.point(0, 0)
    // Nothing but the bar is on the screen: a copy now sees only what's
    // behind.
    property bool quiet: true

    readonly property GlassBackdrop glass: GlassBackdrop {
        window: backdrop.Window.window
        texture: texels
        ready: backdrop.copy?.hasContent ?? false
        origin: backdrop.origin
        screenWidth: backdrop.screen?.width ?? 0
        screenHeight: backdrop.screen?.height ?? 0
        skipTop: Tokens.barGap + Tokens.barHeight
    }

    readonly property string scene: [Compositor.focusedWindow?.id ?? "", Compositor.focusedWindow?.title ?? "", Protocol.focusKey(Compositor.focusedWorkspace)].join("\n")
    property bool wanted: false
    onSceneChanged: {
        wanted = true;
        settle.restart();
    }
    onQuietChanged: {
        if (quiet && wanted)
            settle.restart();
    }
    Timer {
        id: settle
        interval: Tokens.glassBackdropDelayMs
        // Without a first copy there's nothing to take one with yet (no
        // screen capture, or not ready): that first one comes by itself.
        onTriggered: {
            if (!backdrop.quiet || !backdrop.copy?.hasContent)
                return;
            backdrop.wanted = false;
            console.info(`nova: glass copies ${backdrop.screen.name}`);
            backdrop.copy.captureFrame();
        }
    }

    // Only where glass can draw with a shader: Qt Quick's software renderer
    // (the headless e2e run) has none, and its screen copies have no GL.
    readonly property bool shaders: GraphicsInfo.api !== GraphicsInfo.Unknown && GraphicsInfo.api !== GraphicsInfo.Software
    readonly property ScreencopyView copy: copier.item as ScreencopyView
    Loader {
        id: copier
        active: backdrop.shaders
        sourceComponent: ScreencopyView {
            width: Tokens.glassBackdropTexels.width
            height: Tokens.glassBackdropTexels.height
            captureSource: backdrop.screen
            live: false
        }
    }
    ShaderEffectSource {
        id: texels
        sourceItem: backdrop.copy
        hideSource: true
        visible: false
        smooth: true
        textureSize: Tokens.glassBackdropTexels
    }

    onShadersChanged: {
        if (shaders)
            GlassBackdrops.add(glass);
        else
            GlassBackdrops.remove(glass);
    }
    Component.onCompleted: {
        if (shaders)
            GlassBackdrops.add(glass);
    }
    Component.onDestruction: GlassBackdrops.remove(glass)
}
