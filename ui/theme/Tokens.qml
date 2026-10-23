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

    // Liquid glass behind pills and the panels that morph out of them
    // (Glass.qml), over the compositor's blur of what's behind: a light tint
    // with a hint of the accent, so the blurred windows show through.
    readonly property real glassOpacity: 0.40
    readonly property real glassAccent: 0.05
    readonly property color glass: {
        const tinted = tint(primary, glassAccent);
        return Qt.rgba(tinted.r, tinted.g, tinted.b, glassOpacity);
    }
    // A sheen across the top fading into a faint veil, and a shade along the
    // bottom edge.
    readonly property color glassSheen: Qt.rgba(fg.r, fg.g, fg.b, 0.12)
    readonly property color glassVeil: Qt.rgba(fg.r, fg.g, fg.b, 0.03)
    readonly property color glassShade: Qt.rgba(0, 0, 0, 0.22)
    // The rim, lit from the top-left: brightest there, a softer glow at the
    // bottom-right, faint along the sides between.
    readonly property real glassRimWidth: 1.2
    readonly property color glassRimLit: Qt.rgba(fg.r, fg.g, fg.b, 0.60)
    readonly property color glassRimGlow: Qt.rgba(fg.r, fg.g, fg.b, 0.36)
    readonly property color glassRim: Qt.rgba(fg.r, fg.g, fg.b, 0.10)
    readonly property color glassRimDim: Qt.rgba(fg.r, fg.g, fg.b, 0.03)
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
    readonly property int radiusRow: 14
    readonly property int radiusPill: 999

    // Spacing
    readonly property int gap: 8
    readonly property int pad: 12
    readonly property int barHeight: 36
    readonly property int barMargin: 8
    // Icons: in the bar's pills, and in panels.
    readonly property int iconSizeSmall: 14
    readonly property int iconSize: 16

    // Panels: width, inner padding, the gap between sections, list rows and
    // meters (usage bars).
    readonly property int panelWidth: 300
    readonly property int panelPad: 20
    readonly property int panelPadBottom: 16
    readonly property int sectionGap: 18
    readonly property int rowHeight: 40
    readonly property int meterHeight: 8
    readonly property int meterHeightSmall: 4
    // Inner padding of a list row.
    readonly property int rowInset: 10
    // Small labels in a pill shape, like a plan name.
    readonly property int chipHeight: 22
    readonly property int chipPad: 10

    // Type
    readonly property string fontSans: "Inter"
    readonly property string fontMono: "JetBrains Mono"
    readonly property string fontDisplay: "Instrument Serif"
    readonly property int fontSize: 13
    readonly property int fontSizeSmall: 12
    readonly property int fontSizeTiny: 11
    // A panel's headline number.
    readonly property int fontSizeHero: 56
    readonly property int lineHeightHero: 60

    // Motion (ms)
    readonly property int durationFast: 120
    readonly property int durationPanel: 250
    // Content fading in once a panel has grown, and meters filling.
    readonly property int durationFade: 140
    readonly property int durationFill: 600
    readonly property int durationColor: 200
    // One turn of a loading spinner.
    readonly property int durationSpin: 1000
    // The cubic-bezier every panel motion uses (fast out, gentle stop).
    readonly property list<real> easing: [0.2, 0.8, 0.2, 1, 1, 1]

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
