import QtQuick

// Smoked glass's tint where its window has a copy of the screen
// (GlassBackdrops): thick only where what's behind is bright, per pixel
// (glass_tint.frag). `active` says it draws; otherwise whoever holds it fills
// with the fixed Tokens.glass. A rounded rectangle, or with `cutout` set, a
// square with that circle cut out (an edge tab's fillet, EdgeFillet).
ShaderEffect {
    id: effect

    property real radius: 0
    // In px: the circle cut out of a fillet, or none.
    property bool cutout: false
    property point cutCentre: Qt.point(0, 0)
    property real cutRadius: 0

    readonly property GlassBackdrop backdrop: GlassBackdrops.of(Window.window)
    readonly property bool active: backdrop !== null
    visible: active

    // Where it is on the screen, kept up each frame: it moves with whatever
    // it's in (a pill sliding, a panel morphing), which no binding sees. A
    // new place is set before the frame syncs, so it lands in that frame.
    property rect onScreen: Qt.rect(0, 0, 0, 0)
    function track() {
        if (!backdrop)
            return;
        const at = mapToItem(null, 0, 0);
        const x = at.x + backdrop.origin.x;
        const y = at.y + backdrop.origin.y;
        if (x !== onScreen.x || y !== onScreen.y || width !== onScreen.width || height !== onScreen.height)
            onScreen = Qt.rect(x, y, width, height);
    }
    onBackdropChanged: track()
    onWidthChanged: track()
    onHeightChanged: track()
    Connections {
        target: effect.active ? effect.Window.window : null
        function onAfterAnimating() {
            effect.track();
        }
    }

    // The ink's relative luminance (WCAG, sRGB).
    function linear(c: real): real {
        return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
    }
    readonly property real inkLum: 0.2126 * linear(Tokens.fg.r) + 0.7152 * linear(Tokens.fg.g) + 0.0722 * linear(Tokens.fg.b)

    readonly property real screenWidth: Math.max(1, backdrop?.screenWidth ?? 1)
    readonly property real screenHeight: Math.max(1, backdrop?.screenHeight ?? 1)

    // The shader's inputs (glass_tint.frag).
    property variant screenCopy: backdrop?.texture ?? null
    property color tint: Tokens.glassTint
    property rect region: Qt.rect(onScreen.x / screenWidth, onScreen.y / screenHeight, onScreen.width / screenWidth, onScreen.height / screenHeight)
    property vector4d cut: Qt.vector4d(cutCentre.x, cutCentre.y, cutRadius, 0)
    property size size: Qt.size(width, height)
    property real shape: cutout ? 1 : 0
    // A texel below the skipped rows, so smoothing doesn't reach them.
    property real floorY: (backdrop?.skipTop ?? 0) / screenHeight + 1 / Tokens.glassBackdropTexels.height
    property real contrast: Tokens.glassContrast
    property real minAlpha: Tokens.glassMinOpacity
    property real maxAlpha: Tokens.glassMaxOpacity
    property real fallback: Tokens.glassOpacity
    property real ready: backdrop?.ready ? 1 : 0

    fragmentShader: Qt.resolvedUrl("glass_tint.frag.qsb")
}
