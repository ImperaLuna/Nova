pragma Singleton

import QtQuick

// Design tokens from the Nova design language doc. Components read sizes,
// colors and timings from here and never hard-code them.
QtObject {
    // Surfaces
    readonly property color surface: Qt.rgba(0x0E / 255, 0x0F / 255, 0x12 / 255, 0.75)
    readonly property color surfaceRaised: Qt.rgba(1, 1, 1, 0.06)
    readonly property color outline: Qt.rgba(1, 1, 1, 0.08)

    // Text
    readonly property color text: "#ECECF1"
    readonly property color textMuted: "#9A9BA6"

    // Accents
    readonly property color accent: "#8B7CF6"
    readonly property color good: "#5BD69A"
    readonly property color bad: "#F2697B"

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

    // Motion (ms)
    readonly property int durationFast: 120
    readonly property int durationPanel: 250
}
