pragma Singleton

import QtQuick

// Design tokens from the Nova design language doc. Components read sizes,
// colors and timings from here and never hard-code them.
//
// Colors come from the active theme, which novad loads and pushes over the
// socket (themes/<id>.toml). Daemon.qml calls apply() with each theme event;
// nothing is drawn before the first one arrives.
QtObject {
    id: root

    // Theme colors, one per token in nova-protocol's Palette. A novad test
    // checks every token has a property here.

    // Backgrounds
    property color bg
    property color bgAlt

    // Foregrounds
    property color fg
    property color fgAlt
    property color fgDim

    // Accents
    property color primary
    property color secondary
    property color onAccent

    // Status
    property color info
    property color success
    property color warning
    property color error

    // Swatches: distinct colors for things told apart by color, used in order
    property color swatch1
    property color swatch2
    property color swatch3
    property color swatch4
    property color swatch5
    property color swatch6
    property color swatch7
    property color swatch8

    // Glass surfaces, derived from the theme
    readonly property real surfaceOpacity: 0.75
    readonly property real raisedOpacity: 0.06
    readonly property real outlineOpacity: 0.08
    readonly property color surface: Qt.rgba(bg.r, bg.g, bg.b, surfaceOpacity)
    readonly property color surfaceRaised: Qt.rgba(fg.r, fg.g, fg.b, raisedOpacity)
    readonly property color outline: Qt.rgba(fg.r, fg.g, fg.b, outlineOpacity)

    // Glass behind pills and the panels that morph out of them: a dark tint
    // with a hint of the accent and a faint rim. A solid
    // stand-in until the blur spike (IMP-9) picks real blur.
    readonly property real glassOpacity: 0.78
    readonly property real glassAccent: 0.05
    readonly property color glass: {
        const tinted = tint(primary, glassAccent);
        return Qt.rgba(tinted.r, tinted.g, tinted.b, glassOpacity);
    }
    readonly property color glassRim: Qt.rgba(fg.r, fg.g, fg.b, 0.10)
    // A brighter line along the glass's top inner edge.
    readonly property color glassHighlight: Qt.rgba(fg.r, fg.g, fg.b, 0.16)
    // The soft shadow under glass, and the room it needs around the glass.
    readonly property color shadow: Qt.rgba(0, 0, 0, 0.35)
    readonly property real shadowBlur: 30
    readonly property real shadowOffset: 10
    readonly property real shadowRoom: shadowBlur + shadowOffset
    // Hover wash on things inside glass.
    readonly property color hover: Qt.rgba(fg.r, fg.g, fg.b, 0.06)
    // Meters: the empty track and a neutral fill.
    readonly property color track: Qt.rgba(fg.r, fg.g, fg.b, 0.10)
    readonly property color meter: Qt.rgba(fg.r, fg.g, fg.b, 0.85)

    // `target` mixed into bg: 0 is bg, 1 is the color itself. One function for
    // every ramp and tint, so they follow the theme: heatmaps step through
    // tint(primary, level), status rows sit on tint(error, 0.15) and so on.
    function tint(target: color, amount: real): color {
        return Qt.tint(bg, Qt.rgba(target.r, target.g, target.b, amount));
    }

    // Radii
    readonly property int radiusPanel: 24
    readonly property int radiusCard: 16
    readonly property int radiusControl: 12
    readonly property int radiusPill: 999

    // Spacing
    readonly property int gap: 8
    readonly property int pad: 12
    readonly property int barHeight: 36
    readonly property int barMargin: 8

    // Type
    readonly property string fontSans: "Inter"
    readonly property string fontMono: "JetBrains Mono"
    readonly property string fontDisplay: "Instrument Serif"
    readonly property int fontSize: 13
    readonly property int fontSizeSmall: 12
    readonly property int fontSizeTiny: 11

    // Motion (ms)
    readonly property int durationFast: 120
    readonly property int durationPanel: 250

    // Sets the theme colors from a theme event's `colors` ({token: "#RRGGBB"}).
    function apply(colors) {
        for (const token in colors) {
            if (token in root)
                root[token] = colors[token];
            else
                console.warn(`nova: theme token ${token} has no property in Tokens.qml`);
        }
    }
}
