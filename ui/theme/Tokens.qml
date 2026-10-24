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
    // Behind everything when nothing else is drawn, like the lock screen
    property color backdrop

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
    // Where, top to bottom, the sheen has faded to the veil, and where the
    // veil has cleared before the shade starts.
    readonly property real glassVeilAt: 0.45
    readonly property real glassClearAt: 0.85
    // The rim, lit from the top-left: brightest there, a softer glow at the
    // bottom-right, faint along the sides between.
    readonly property real glassRimWidth: 1.2
    // Where, along the diagonal, the lit corner has faded to the plain rim
    // (and, mirrored, where the glow corner starts).
    readonly property real glassRimFade: 0.28
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
    // What's selected where the focus isn't, like the workspace shown on a
    // monitor you're not using: a quiet fill instead of the accent.
    readonly property color selectedQuiet: Qt.rgba(fg.r, fg.g, fg.b, 0.14)
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
    // The central pill's workspace numbers: each a round cell this big, this
    // far apart, and the line between them and the day.
    readonly property int workspaceSize: 26
    readonly property int workspaceGap: 2
    readonly property int dividerHeight: 16
    readonly property int dividerWidth: 1
    // Small marks, like a workspace's "wants attention" dot.
    readonly property int dotSize: 4
    // Icons: in the bar's pills, and in panels.
    readonly property int iconSizeSmall: 14
    readonly property int iconSize: 16
    // Between an icon and the label or number beside it.
    readonly property int iconGap: 6
    // Thin strokes, like a ring or a spinner's arc.
    readonly property real strokeThin: 1.5
    // Status rings in an icon box: kept this far inside its edge, dashed when
    // something is unavailable, a short arc spinning while it loads (degrees).
    readonly property int ringInset: 2
    readonly property list<real> ringDashes: [2, 2]
    readonly property real spinnerSweep: 86
    // A label beside an icon sits this far right, to line up optically with
    // the icon's edge.
    readonly property int labelNudge: 1
    // Something that can't be used right now, like an unavailable provider.
    readonly property real dimOpacity: 0.5

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

    // Lock screen: the circle with the user's initial, the password field
    // and its dots, and the space between them.
    readonly property int lockAvatarSize: 72
    readonly property int lockInitialSize: 28
    readonly property int lockFieldWidth: 280
    readonly property int lockFieldHeight: 44
    readonly property int lockDotSize: 8
    readonly property int lockDotGap: 6
    readonly property int lockGap: 16
    // How far a wrong password shakes the field.
    readonly property int lockShake: 8
    // Half-swings in a shake (each one smaller than the last).
    readonly property int lockShakeSwings: 4
    // The field's ring while it has focus.
    readonly property real lockFocusOpacity: 0.75

    // The volume capsule (ui/volume): a glass capsule with liquid inside that
    // slides in from the right edge, this far from it.
    readonly property int capsuleWidth: 48
    readonly property int capsuleHeight: 260
    readonly property int capsuleEdgeGap: 12
    // Glass between the rim and the liquid.
    readonly property int capsuleWall: 4
    // The speaker and number sit this far above the capsule's bottom.
    readonly property int capsuleLabelPad: 13
    // They fade in over this last share of its way out.
    readonly property real capsuleLabelFadeIn: 0.4
    // While it slides it stretches (thinner and taller) by up to this share,
    // and it overshoots its full height by up to this share as it lands.
    readonly property real capsuleStretch: 0.10
    readonly property real capsuleOvershoot: 0.08
    // How long it stays out after the last change, and how much one scroll
    // notch or volume key changes the volume (1 = 100%).
    readonly property int durationCapsuleHold: 1500
    readonly property real volumeStep: 0.05
    // The liquid, and the grey still liquid of a muted output.
    readonly property real liquidOpacity: 0.95
    readonly property color liquid: Qt.rgba(secondary.r, secondary.g, secondary.b, liquidOpacity)
    readonly property real liquidMutedOpacity: 0.7
    readonly property color liquidMuted: Qt.rgba(fgDim.r, fgDim.g, fgDim.b, liquidMutedOpacity)
    // Its surface (px): a slow swell, the waves a change stirs up (at most),
    // the swell kept while it sits still, and how far the surface creeps
    // up the glass.
    readonly property real liquidSwell: 1.8
    readonly property real liquidSloshMax: 6
    readonly property real liquidIdleWave: 1.2
    readonly property real liquidMeniscus: 1.6
    // How fast stirred waves settle (s), and how strongly the level's speed
    // stirs them.
    readonly property real liquidSloshDecay: 0.45
    readonly property real liquidSloshGain: 32
    // Specks drifting up through it: most at once, and the share always
    // rising while it's calm.
    readonly property int liquidSpecks: 14
    readonly property real liquidFizz: 0.3
    // Light inside it: just under the surface, and down the left side.
    readonly property real liquidGlow: 0.28
    readonly property real liquidStreak: 0.16
    // Springs (stiffness, damping ratio): the capsule sliding, the level
    // following the volume, and the surface swinging back after a tilt.
    readonly property real springCapsule: 170
    readonly property real springCapsuleDamping: 0.62
    readonly property real springLevel: 70
    readonly property real springLevelDamping: 0.85
    readonly property real springTilt: 90
    readonly property real springTiltDamping: 0.18
    // How much the capsule's sideways acceleration tilts the surface, and
    // the steepest tilt (px per px).
    readonly property real liquidTiltGain: 0.005
    readonly property real liquidTiltMax: 0.5

    // Type
    readonly property string fontSans: "Inter"
    readonly property string fontMono: "JetBrains Mono"
    readonly property string fontDisplay: "Instrument Serif"
    readonly property int fontSize: 13
    readonly property int fontSizeSmall: 12
    readonly property int fontSizeTiny: 11
    // A name or heading on its own, like the user's on the lock screen.
    readonly property int fontSizeTitle: 18
    // The time in the central pill.
    readonly property int fontSizeTime: 14
    // A panel's headline number.
    readonly property int fontSizeHero: 56
    readonly property int lineHeightHero: 60
    // The headline's tracking, as a share of its size (tighter when big), and
    // the size of a unit (like %) beside it, relative to its digits.
    readonly property real trackingHero: -0.02
    readonly property real heroUnitScale: 0.42
    // Line height for text that wraps, like a hint under a meter.
    readonly property real lineHeightWrapped: 1.5

    // Motion (ms)
    readonly property int durationFast: 120
    readonly property int durationPanel: 250
    // Content fading in once a panel has grown, and meters filling.
    readonly property int durationFade: 140
    readonly property int durationFill: 600
    readonly property int durationColor: 200
    // One turn of a loading spinner.
    readonly property int durationSpin: 1000
    // A wrong password's shake, and the lock screen fading before it unlocks.
    readonly property int durationShake: 360
    readonly property int durationUnlock: 400
    // The scale something new grows in from.
    readonly property real enterScale: 0.6
    // One breath of something that pulses for attention.
    readonly property int durationPulse: 1600
    // The cubic-bezier every panel motion uses (fast out, gentle stop).
    readonly property list<real> easing: [0.2, 0.8, 0.2, 1, 1, 1]
    // The curve each half of a pulse follows: a soft breath in and out.
    readonly property int easingPulse: Easing.InOutSine

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
