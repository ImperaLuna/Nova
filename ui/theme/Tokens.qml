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
    // Glass, laid translucent over the compositor's blur (see `glass`)
    property color glassTint

    // Foregrounds: text and icons. The dimmer levels are this ink at less
    // opacity, so over glass they take on what's behind like the ink does.
    property color fg
    readonly property real fgDimOpacity: 0.66
    readonly property color fgDim: alpha(fg, fgDimOpacity)

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
    readonly property real raisedOpacity: 0.09
    readonly property real outlineOpacity: 0.08
    readonly property color surface: alpha(bg, surfaceOpacity)
    readonly property color surfaceRaised: alpha(fg, raisedOpacity)
    readonly property color outline: alpha(fg, outlineOpacity)

    // Smoked glass behind pills and the panels that morph out of them
    // (Glass.qml), over the compositor's blur of what's behind: a warm
    // near-black that only darkens what's behind, so its colors still show.
    readonly property real glassOpacity: 0.55
    readonly property color glass: alpha(glassTint, glassOpacity)
    // Where the window has a copy of the screen (GlassTint), glass thickens
    // over bright spots instead: as little tint as keeps the ink at
    // `glassContrast` : 1 (WCAG), within these bounds. `glassOpacity` until
    // the first copy. A copy is taken this long after what's on the screen
    // last changed, and again this much later once windows have settled, at
    // this many texels.
    readonly property real glassMinOpacity: 0.35
    readonly property real glassMaxOpacity: 0.90
    readonly property real glassContrast: 7
    readonly property int glassBackdropDelayMs: 300
    readonly property int glassBackdropSettleMs: 700
    readonly property size glassBackdropTexels: Qt.size(64, 36)
    // Its edge: a dark outline outside, a faint light line inside, and a
    // highlight along the top that fades out before the corners (where it
    // starts, as a share of the corner radius).
    readonly property color glassOutline: Qt.rgba(0, 0, 0, 0.45)
    readonly property color glassInnerLine: alpha(fg, 0.06)
    readonly property color glassHighlight: alpha(fg, 0.22)
    readonly property real glassHighlightInset: 0.7
    readonly property real glassHighlightFade: 0.15
    readonly property real glassEdgeWidth: 1
    // The volume capsule's glass (Capsule.qml): a sheen across the top and a
    // rim lit from the top-left, brightest there, a softer glow at the
    // bottom-right; `glassRimShade` is its unlit side, relative to the lit
    // corner.
    readonly property color glassSheen: alpha(fg, 0.12)
    readonly property real glassVeilAt: 0.45
    readonly property real glassRimWidth: 1.2
    readonly property real glassRimShade: 0.35
    readonly property color glassRimLit: alpha(fg, 0.60)
    readonly property color glassRimGlow: alpha(fg, 0.36)
    // A hairline around things inside glass.
    readonly property color glassRim: alpha(fg, 0.10)
    // The soft shadow under glass, and the room it needs around the glass.
    readonly property color shadow: Qt.rgba(0, 0, 0, 0.35)
    readonly property real shadowBlur: 30
    readonly property real shadowOffset: 10
    readonly property real shadowRoom: shadowBlur + shadowOffset
    // Hover wash on things inside glass.
    readonly property color hover: alpha(fg, 0.06)
    // The selected row of a list, like the launcher's.
    readonly property color selected: alpha(fg, 0.18)
    // What's selected where the focus isn't, like the workspace shown on a
    // monitor you're not using: a quiet fill instead of the accent.
    readonly property color selectedQuiet: alpha(fg, 0.14)
    // Meters: the empty track and a neutral fill.
    readonly property color track: alpha(fg, 0.14)
    readonly property color meter: alpha(fg, 0.85)

    // `target` mixed into bg: 0 is bg, 1 is the color itself. One function for
    // every ramp and tint, so they follow the theme: heatmaps step through
    // tint(primary, level), status rows sit on tint(error, 0.15) and so on.
    function tint(target: color, amount: real): color {
        return Qt.tint(bg, alpha(target, amount));
    }

    // Radii
    readonly property int radiusPanel: 24
    readonly property int radiusCard: 16
    readonly property int radiusControl: 12
    readonly property int radiusRow: 14

    // Spacing
    readonly property int gap: 8
    readonly property int pad: 12
    readonly property int barHeight: 36
    // The bar's pills: this far in from the screen's sides, and this far
    // from its top edge and from the windows below (the space kept free is
    // barGap + barHeight + barGap).
    readonly property int barMargin: 8
    readonly property int barGap: 4
    // While the bar steps aside for a fullscreen window: the strip along the
    // top edge that brings it back when the pointer touches it.
    readonly property int revealStrip: 2
    // The central pill's workspace numbers: each a round cell this big, this
    // far apart, and the line between them and the day.
    readonly property int workspaceSize: 26
    readonly property int workspaceGap: 2
    // Extra room between the special workspaces and the numbers after them.
    readonly property int workspaceGroupGap: 4
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

    // Power menu: a tab grown out of the right screen edge, joined to it by
    // concave fillets, from a nub of nubHeight. Rows like a list's, a bit
    // taller, with bigger icons.
    readonly property int edgeTabRadius: 27
    readonly property int edgeTabFillet: 22
    readonly property int edgeTabNub: 40
    readonly property int powerMenuWidth: 220
    readonly property int powerRowHeight: 46
    readonly property int powerRowGap: 4
    readonly property int iconSizeMenu: 18
    // Line icons' stroke on their 24-unit grid.
    readonly property real iconStroke: 1.8
    // The selected row's accent fill, and the bar draining while a row
    // waits for its second click.
    readonly property real accentFillOpacity: 0.92
    readonly property int drainHeight: 3
    readonly property real drainOpacity: 0.55
    // Rows that can't run right now, and the others while one runs.
    readonly property real busyOpacity: 0.35

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
    // all of it from this speed on (its way out per second), and it
    // overshoots its full height by up to this share as it lands.
    readonly property real capsuleStretch: 0.10
    readonly property real capsuleStretchSpeed: 6
    readonly property real capsuleOvershoot: 0.08
    // How long it stays out after the last change, and how much one scroll
    // notch or volume key changes the volume (1 = 100%).
    readonly property int durationCapsuleHold: 1500
    readonly property real volumeStep: 0.05
    // The liquid, and the grey still liquid of a muted output.
    readonly property real liquidOpacity: 0.95
    readonly property color liquid: alpha(secondary, liquidOpacity)
    readonly property real liquidMutedOpacity: 0.7
    readonly property color liquidMuted: alpha(fg, fgDimOpacity * liquidMutedOpacity)
    // Its surface (px): a slow swell, the waves a change stirs up (at most),
    // the swell kept while it sits still, and how far the surface creeps
    // up the glass.
    readonly property real liquidSwell: 1.8
    readonly property real liquidSloshMax: 6
    readonly property real liquidIdleWave: 1.2
    readonly property real liquidMeniscus: 1.6
    // How far from the glass the meniscus fades out (px).
    readonly property real liquidMeniscusReach: 5
    // How fast stirred waves settle (s), and how strongly the level's speed
    // stirs them.
    readonly property real liquidSloshDecay: 0.45
    readonly property real liquidSloshGain: 32
    // Specks drifting up through it: most at once, and the share always
    // rising while it's calm.
    readonly property int liquidSpecks: 14
    readonly property real liquidFizz: 0.3
    // A speck's radius (px): the smallest, and how much bigger one can be;
    // how bright they glow, and the brightest a cluster gets (0..1).
    readonly property real liquidSpeckRadius: 0.8
    readonly property real liquidSpeckRadiusRange: 1.5
    readonly property real liquidSpeckGlow: 0.7
    readonly property real liquidSpeckGlowMax: 0.85
    // How much quicker they rise at full fizz (2: three times as fast).
    readonly property real liquidFizzPace: 2
    // Light inside it: just under the surface, and down the left side.
    readonly property real liquidGlow: 0.28
    readonly property real liquidStreak: 0.16
    // How deep the light under the surface reaches, and how far down the
    // streak takes to grow in (px); where the streak runs across the
    // liquid and how wide it is (shares of its width); the light's color.
    readonly property real liquidGlowDepth: 6
    readonly property real liquidStreakFade: 18
    readonly property real liquidStreakAt: 0.18
    readonly property real liquidStreakWidth: 0.07
    readonly property color liquidLight: "white"
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

    // The right pill: round buttons inside it, this far from its ends and
    // each other.
    readonly property int pillButtonSize: 28
    readonly property int pillButtonInset: 4
    readonly property int pillButtonGap: 2

    // The tray (ui/tray), folded into the right pill behind a chevron: at
    // most this many icons show, the rest scroll, the strip's ends fade
    // over this width, and each icon is this big. An app asking for
    // attention gets a dot this big, ringed in the bar's color.
    readonly property int trayVisible: 5
    readonly property int trayFade: 22
    readonly property int trayIconSize: 18
    readonly property int trayIconRadius: 5
    readonly property int trayDotSize: 7
    readonly property int trayDotRing: 2
    // How much later each icon pops in after the one before it.
    readonly property int trayStagger: 28
    // An app's menu: at least this wide, its rows and corners.
    readonly property int menuWidth: 230
    readonly property int menuPad: 6
    readonly property int menuRowHeight: 32
    readonly property int menuRowRadius: 10
    readonly property int menuCheckSize: 14
    readonly property int menuCheckRadius: 4
    readonly property int menuCheckIconSize: 11
    readonly property int menuSeparatorInset: 6
    readonly property int menuSeparatorGap: 4
    readonly property int menuGap: 8
    // Quick settings' Tray row: icons in a grid this many across, each
    // cell this tall.
    readonly property int trayGridColumns: 8
    readonly property int trayGridCell: 36
    readonly property int trayGridIconSize: 20
    readonly property int trayGridGap: 4
    // Rows the quick settings tray shows before it scrolls.
    readonly property int trayGridMaxRows: 3

    // Quick settings (ui/quick_settings): a glass drawer this wide against
    // the screen's right edge, top to bottom. Its pages have this padding
    // and gap between sections, and slide this far as they swap.
    readonly property int quickSettingsWidth: 368
    readonly property int quickSettingsPad: 16
    readonly property int quickSettingsGap: 12
    readonly property int pageSlide: 28
    readonly property int durationPage: 280
    // Its sections fade in one after another as it opens: the first at
    // this share of the morph, each next one this much later.
    readonly property real revealFirst: 0.35
    readonly property real revealStagger: 0.14
    // How far each section rises into place as it fades in.
    readonly property int revealRise: 6
    // The header: the distro logo on its own, the greeting beside it.
    readonly property int logoSize: 36
    // Toggle tiles, two per row, with an arrow to a page on some.
    readonly property int tileHeight: 58
    readonly property int tileArrowWidth: 22
    // The Bluetooth page: a pairing code in a row of cells, the field a PIN
    // is typed in, and the tint of the cards under a row.
    readonly property int pairingCellWidth: 28
    readonly property int pairingCellHeight: 36
    readonly property int pairingCellGap: 4
    readonly property int pairingFieldHeight: 34
    readonly property real pairingTintOpacity: 0.10
    // Thick sliders: a round icon button (mute) beside the track, the track
    // filled only to the value, and a button after it that opens a page. An
    // app's slider shows the app's icon, its corners less round.
    readonly property int sliderHeight: 34
    readonly property int sliderGap: 6
    readonly property color sliderFill: alpha(fg, 0.22)
    readonly property real sliderMutedOpacity: 0.3
    readonly property int sliderButtonWidth: 30
    readonly property int appIconRadius: 11
    readonly property real appMutedOpacity: 0.45
    // The mic slider while an app records: the second accent.
    readonly property real micFillOpacity: 0.34
    // The Devices card: a row per wireless device, its link in a small
    // chip, and a little battery (body, the nub on its right, the gap
    // between its rim and the fill).
    readonly property int deviceRowHeight: 36
    // Rows the card shows before it scrolls.
    readonly property int deviceListMaxRows: 4
    readonly property int deviceIconSize: 17
    readonly property int deviceLinkIconSize: 10
    readonly property int deviceChipHeight: 18
    readonly property int deviceChipPad: 7
    readonly property int deviceChipGap: 4
    readonly property int deviceBatteryWidth: 22
    readonly property int deviceBatteryHeight: 11
    readonly property int deviceBatteryRadius: 3
    readonly property int deviceBatteryInset: 2
    readonly property int deviceBatteryNubWidth: 2
    readonly property int deviceBatteryNubHeight: 4
    readonly property real deviceBatteryRim: 1.25
    readonly property real deviceBatteryRimOpacity: 0.55
    readonly property int deviceBoltSize: 9
    // Room for "100%" (or "High"), so the batteries line up.
    readonly property int deviceLevelWidth: 34
    // Resource rings, and the labels under them.
    readonly property int ringSize: 52
    readonly property int ringStroke: 4
    // Small uppercase labels spread out by this share of their size.
    readonly property real trackingLabel: 0.06
    readonly property color fgSoft: alpha(fg, 0.72)
    // A segmented control or tab row: its padding, its segments' height and
    // corners.
    readonly property int segmentPad: 4
    readonly property int segmentHeight: 30
    readonly property int radiusSegment: 10
    // The resources page: its graph's height and its process rows.
    readonly property int graphHeight: 72
    readonly property int graphLine: 2
    readonly property real graphAreaOpacity: 0.22
    readonly property int processRowHeight: 30
    readonly property int processNumberWidth: 64
    // The resources page's End button (✕ on a row's hover, in the second
    // accent), its confirm row's padding, and the search field's height.
    readonly property int endButtonWidth: 26
    readonly property int endButtonHeight: 24
    readonly property int endIconSize: 13
    readonly property real endHoverOpacity: 0.22
    readonly property real confirmOpacity: 0.14
    readonly property int confirmButtonHeight: 26
    readonly property int searchHeight: 34
    // A legend's colored dot.
    readonly property int legendDot: 7
    // The network page: its traffic card's speeds and graph, a link's role
    // chip ("In use", "Backup"), how faint Wi-Fi's arcs are that a weak
    // signal doesn't reach, and a network row's ⋯ menu.
    readonly property int trafficGraphHeight: 54
    readonly property real wifiArcFadedOpacity: 0.3
    readonly property int networkMenuWidth: 170
    // The launcher (ui/launcher): a deck in the middle of the screen, this
    // share of the screen's height from the top, that fades in at
    // launcherEnterScale and launcherEnterRise above its place. A search
    // field, then the pinned apps as tiles (the name shows on hover), then a
    // list of rows, at most launcherRows of them before it scrolls.
    readonly property int launcherWidth: 620
    readonly property real launcherTop: 0.17
    readonly property real launcherEnterScale: 0.96
    readonly property int launcherEnterRise: 8
    readonly property int launcherPad: 14
    readonly property int launcherGap: 10
    readonly property int launcherSearchHeight: 44
    readonly property int launcherLabelHeight: 16
    readonly property int launcherTileSize: 64
    readonly property int launcherTileIcon: 38
    readonly property int launcherTileName: 16
    readonly property int launcherRowHeight: 46
    readonly property int launcherRowGap: 2
    readonly property int launcherRowIcon: 30
    readonly property int launcherRows: 6
    // The Apps and Files tabs in the search field, with Nova Files on.
    readonly property int launcherModesWidth: 132
    // The note shown when nothing matches.
    readonly property int launcherEmptyHeight: 92
    // An app without an icon: its letter on a swatch, this round.
    readonly property int letterTileRadius: 9
    // The letter's size, as a share of the tile's.
    readonly property real letterScale: 0.47
    // The pin button on a row.
    readonly property int pinButtonSize: 24
    // The text cursor in a field: the accent, and steady, so an open field
    // doesn't redraw twice a second.
    readonly property int caretWidth: 2

    // Screenshots: the dim over the frozen screens outside the region, the
    // region's outline, the toolbar at the top (the
    // central pill grown into it), and the card a saved shot lands in.
    readonly property real captureVeilOpacity: 0.45
    readonly property color captureVeil: Qt.rgba(0, 0, 0, captureVeilOpacity)
    readonly property real captureOutline: 1.5
    // Smaller than this (logical px) either way is a click, not a region.
    readonly property int captureMinDrag: 4
    readonly property int captureBarHeight: 44
    readonly property int captureButtonHeight: 32
    readonly property int captureButtonPad: 12
    // The camera/video switch: two buttons' width, the knob inset by the pad.
    readonly property int captureSwitchWidth: 76
    readonly property int captureSwitchPad: 2
    readonly property int captureBadgeHeight: 26
    readonly property int shotCardWidth: 360
    readonly property int shotThumbHeight: 184
    readonly property int shotButtonHeight: 36
    readonly property int shotCardGap: 12
    // How long the card stays; the pointer on it holds it.
    readonly property int durationShotCard: 6000
    // A recording that didn't start: longer, as there's a reason to read.
    readonly property int durationFailedCard: 10000
    // The reason's lines at most, on that card.
    readonly property int failedCardLines: 3
    // Recording: the count before it starts, shown big in the middle of
    // what's recorded. The pause after the picker goes so it isn't in what's
    // captured next: a recording's first frames, or a screenshot's frozen
    // frame after switching from video.
    readonly property int recordCountdown: 3
    readonly property int durationCountdownStep: 1000
    readonly property int pickerGoneDelay: 150
    // The red dot in the record button and the recording pill.
    readonly property int recordDotSize: 10
    // The marks just outside a recorded region's corners.
    readonly property int recordMarkLength: 18
    readonly property int recordMarkWidth: 3
    readonly property int recordMarkGap: 4

    // Notifications (ui/notifications): pop-ups this wide under the right
    // pill, at most notificationsShown at once, each staying
    // durationNotification unless its app asks otherwise; several coming
    // at once open into their slots notificationStagger ms apart. Who it's
    // from (a picture, or else the app's icon), its lines, buttons,
    // progress and the line draining its time; critical ones get a rim.
    readonly property int notificationWidth: 380
    readonly property int notificationsShown: 5
    readonly property int notificationStagger: 60
    readonly property int durationNotification: 6000
    readonly property int radiusNotification: 20
    readonly property int notificationPadBottom: 14
    readonly property int notificationAvatar: 40
    readonly property int notificationAppLine: 18
    readonly property int notificationLineGap: 2
    readonly property int notificationBodyLines: 3
    readonly property int notificationActionsGap: 8
    readonly property int notificationActionHeight: 28
    readonly property int notificationProgressHeight: 6
    readonly property int notificationTimerHeight: 2
    // A card's time left (Countdown) moves every this many ms (12 a
    // second): smooth to the eye, a few frames, not one each vblank.
    readonly property int countdownTick: 83
    readonly property color notificationTimer: alpha(fg, 0.35)
    readonly property real notificationCriticalRim: 1.5
    // A low battery alert's icon: the device's glyph on a rounded tile, its
    // radius and the glyph a share of the icon's size.
    readonly property real notificationDeviceRadius: 0.3
    readonly property real notificationDeviceGlyph: 0.55
    // Counts in the list: its heading's, and "+2" on a stack of an app's.
    readonly property int notificationChipHeight: 16
    readonly property int notificationChipPad: 6
    // The ✕ and ⋯ on a card's corner, shown on hover.
    readonly property int notificationCloseSize: 24
    readonly property int notificationCloseInset: 9
    // A pop-up grows out of the quick settings button: from this share of
    // its width and height, this far above its place. One that's clicked
    // or dismissed slides this far right as it fades.
    readonly property real notificationFold: 0.08
    readonly property real notificationFoldHeight: 0.15
    readonly property int notificationRise: 38
    readonly property int notificationAway: 60
    // The list in quick settings: the heading's round buttons, its rows
    // (an icon, two lines of body), the gap between them, a stack's edges
    // under its top one, the ⋯ menu and the empty list's bell.
    readonly property int notificationButton: 26
    readonly property int notificationRowIcon: 28
    readonly property int notificationRowLines: 2
    readonly property int notificationListGap: 6
    readonly property int notificationPeek: 6
    readonly property int notificationPeekHeight: 14
    readonly property int notificationPeekInset: 10
    readonly property int notificationMenuWidth: 170
    readonly property int notificationEmptyIcon: 28
    // On/off switches, like an app's on the notification settings page.
    readonly property int switchWidth: 40
    readonly property int switchHeight: 24
    readonly property int switchKnob: 18
    // The badge on the quick settings button: unread notifications (up to
    // badgeMax, then "9+"), or a grey crossed-out bell during do not
    // disturb. It sits this far past the button's corner, ringed in bg.
    readonly property int badgeSize: 15
    readonly property int badgePad: 4
    readonly property int badgeOffset: 3
    readonly property int badgeRing: 2
    readonly property int badgeIcon: 9
    readonly property int badgeMax: 9
    readonly property color badgeQuiet: alpha(fg, 0.55)

    // Now playing (ui/media): a pill right of the AI usage pill while
    // something plays, which grows into a panel (panelWidth wide). The pill:
    // the app's icon in a circle (a workspace number's size), the title and
    // artist cut to these widths (a long title fades out over mediaFade and
    // scrolls through at mediaMarqueeSpeed px/s, resting this long at each
    // end), and an equalizer of mediaEqBars bars stepping every
    // durationEqStep, each between mediaEqLow and mediaEqHigh tall.
    readonly property int mediaArt: 26
    readonly property int mediaAppIcon: 22
    readonly property int mediaPillStart: 5
    readonly property int mediaTitleMax: 150
    readonly property int mediaArtistMax: 96
    readonly property int mediaFade: 18
    readonly property int mediaMarqueeSpeed: 30
    readonly property int durationMarqueeRest: 800
    readonly property int mediaEqBars: 4
    readonly property int mediaEqBarWidth: 3
    readonly property int mediaEqGap: 2
    readonly property int mediaEqLow: 3
    readonly property int mediaEqHigh: 12
    readonly property int mediaEqLead: 2
    readonly property int durationEqStep: 125
    // Paused: the app's icon dims to this; after durationMediaLinger the
    // pill slides away, unless its panel is open.
    readonly property real mediaPausedOpacity: 0.6
    readonly property int durationMediaLinger: 60000
    // The panel: the cover and the gap beside it, the title's line height,
    // the progress line (taller on hover, with a knob) and the room it
    // takes clicks in, the buttons and the player chips.
    readonly property int mediaCover: 76
    readonly property int mediaBodyGap: 14
    readonly property real mediaTitleLine: 1.2
    readonly property int mediaSeekRoom: 16
    readonly property int mediaTrack: 4
    readonly property int mediaTrackHover: 6
    readonly property int mediaKnob: 12
    readonly property int mediaPlay: 48
    readonly property int mediaControl: 40
    readonly property int mediaControlGap: 18
    readonly property int mediaControlIcon: 18
    readonly property int mediaPlayIcon: 20
    readonly property int mediaChipHeight: 28
    readonly property int mediaChipPad: 12
    readonly property int mediaChipGap: 6
    readonly property int mediaChipDot: 5
    readonly property int mediaGoToIcon: 18
    readonly property int mediaPadBottom: 18
    // The panel's smaller spacing: under the header, above the buttons;
    // between the title, artist and album; above the times; and above the
    // chips and the go-to row.
    readonly property int mediaGap: 10
    readonly property int mediaMetaGap: 2
    readonly property int mediaTimesGap: 6
    readonly property int mediaRowGap: 14
    // A button the player can't act on now (no next track).
    readonly property real mediaDisabledOpacity: 0.35
    // How often the open panel's time counts up, and how long the progress
    // line stays where a seek put it if the player doesn't say it moved.
    readonly property int durationProgressStep: 1000
    readonly property int durationSeekHold: 2000

    // The wallpaper carousel (ui/wallpaper): over the whole screen, which
    // previews the picked image, a deck of the wallpaper folder's images in
    // the middle that loops both ways. The middle card is this share of the
    // screen's width, shaped like the screen; its neighbours sit this many
    // card widths out, tilted away (degrees), smaller and dimmer, and the
    // ones past them each this much further and smaller. This many show
    // either side; the picked one has a ring.
    readonly property real carouselCardShare: 0.26
    readonly property real carouselNeighbour: 0.74
    readonly property real carouselStep: 0.27
    readonly property real carouselTilt: 50
    readonly property real carouselSideScale: 0.86
    readonly property real carouselDepthScale: 0.06
    readonly property real carouselSideDim: 0.28
    readonly property int carouselSides: 3
    readonly property int carouselRing: 3
    // Opening, the deck rises this far into place and its cards spread out
    // from this share of their distance.
    readonly property int carouselEnterRise: 24
    readonly property real carouselEnterSpread: 0.6
    // Under the deck, the picked image's name and where the pick goes.
    readonly property int carouselLabelGap: 28
    readonly property int carouselLabelHeight: 40
    readonly property int carouselLabelPad: 16
    readonly property int carouselChipHeight: 28
    // With more than one screen, the label also holds the screen picker:
    // every screen drawn to scale, the widest this wide, apart by this gap,
    // the ones not picked for faded to this; and the label grows to fit
    // their names. The screen picked for gets a ring this wide round its
    // edge, and its mini one this wide.
    readonly property int carouselPickerHeight: 64
    readonly property int carouselMiniWidth: 52
    readonly property int carouselMiniGap: 10
    readonly property int carouselMiniRadius: 4
    readonly property int carouselMiniRing: 2
    readonly property real carouselMiniIdle: 0.7
    readonly property int carouselScreenRing: 4
    // The note an empty folder shows instead.
    readonly property int carouselNoteWidth: 420
    // A shade from this far down the preview to its bottom, so the label
    // reads on a bright picture.
    readonly property real carouselShadeFrom: 0.3
    readonly property color carouselShade: Qt.rgba(0, 0, 0, 0.55)
    // A card whose image is still loading.
    readonly property color carouselLoading: alpha(fg, 0.07)

    // Nova Files: an app window with square edges (the compositor rounds
    // windows), its size when it opens, the navigation row (back, forward,
    // up, the address and search fields) and the command row under it, the
    // sidebar of places, and the file list's header with its Modified, Type
    // and Size columns (the sorted one with a small chevron). Rows and
    // fields use the radius of a menu row and of a control; the fields sit
    // darker than the glass.
    readonly property int filesWindowWidth: 880
    readonly property int filesWindowHeight: 700
    // The corner radius packaging/compositor's niri.kdl and hyprland.conf
    // suggest for the window, which its blur region follows.
    readonly property int filesWindowRadius: radiusCard
    readonly property int filesButtonSize: 30
    readonly property int filesFieldHeight: 32
    readonly property int filesSearchWidth: 220
    readonly property color filesField: Qt.rgba(0, 0, 0, 0.22)
    readonly property int filesRowPadY: 6
    readonly property int filesCommandPadTop: 4
    readonly property int filesSeparatorHeight: 16
    readonly property int filesSidebarWidth: 220
    readonly property int filesPlaceHeight: 26
    readonly property int filesHeaderHeight: 30
    readonly property list<int> filesColumns: [150, 110, 80]
    // The same beside the details or preview pane: Type (0) gives way so names fit.
    readonly property list<int> filesColumnsCompact: [90, 0, 70]
    readonly property color filesFaint: alpha(fg, 0.45)
    // A drive in the sidebar: a taller row with how full it is under its
    // name, bar then free space. A stick that's plugged in slides in, then
    // glows in the accent and fades back. Problems with one show in a
    // dialog this wide.
    readonly property int filesDriveHeight: 54
    readonly property int filesDriveBarHeight: 4
    readonly property int filesDriveBarGap: 4
    // What uses space: the drive's bar over the list, thicker, and each
    // row's bar in the column Modified and Type leave.
    readonly property int filesSpaceBarHeight: 8
    readonly property int filesSpaceRowBarHeight: 6
    readonly property int filesSpacePad: 10
    readonly property int filesSpaceBarWidth: 220
    readonly property int durationDriveArrive: 420
    readonly property int durationDriveGlow: 1600
    readonly property real filesDriveGlow: 0.22
    readonly property int filesDialogWidth: 400
    readonly property int filesDialogPad: 22
    readonly property int filesDialogButtonPad: 16
    // The file list's rows: a folder's icon in its own warm color (dimmer
    // for a dot folder), and rows easing up into place one after another
    // when a folder's entries come in.
    readonly property int filesRowHeight: 32
    readonly property color filesFolder: "#E8A35A"
    readonly property real filesHiddenIconOpacity: 0.6
    readonly property int filesRowRise: 4
    readonly property int filesRowStagger: 25
    readonly property int durationRowIn: 220
    // How often the Modified column looks at the clock, so "Today" moves on.
    readonly property int filesClockTick: 60000
    // A pause this long in typing a name in a file list starts a new name,
    // as in Explorer.
    readonly property int filesTypingPause: 1000
    // Solid Nova Files ([files] in config.toml) ends at the theme's soft
    // greys: the frame a step above bg, the file list at bg. On the way the
    // raised rows fade into the frame.
    readonly property color filesSolidFrame: tint(fg, 0.025)
    readonly property color filesSolidList: bg
    // The small chevron beside New, Sort and View.
    readonly property int iconSizeMini: 12
    readonly property real filesChevronOpacity: 0.6
    // The trash: its columns after the name (Original location, Date
    // deleted, Size), the dialog asking before deleting for good (Yes and
    // No at least this wide, the one item it names with a big icon), and
    // the notice at the bottom right when an item can't go back.
    readonly property list<int> filesTrashColumns: [190, 120, 70]
    readonly property int filesConfirmWidth: 400
    readonly property int filesDialogButtonMinWidth: 72
    readonly property int filesNoticeWidth: 330
    // The address bar: each folder's part, washed on hover, and the ring
    // while a path is typed (in the error color for one that isn't a folder).
    readonly property int filesCrumbPadY: 4
    readonly property int filesCrumbRadius: 8
    readonly property real filesFocusRing: strokeThin
    // Search: the field widens while it's in use and the address bar fades
    // back; typing settles this long before novad is asked. Results show
    // where each lives in a column this wide, the matched words in the
    // accent, and the scope (this folder or everywhere) as round chips.
    readonly property int filesSearchWidthOpen: 300
    readonly property real filesSearchCrumbsOpacity: 0.45
    readonly property int filesSearchSettle: 60
    readonly property int filesSearchLocationWidth: 260
    readonly property int filesScopeHeight: 26
    readonly property int filesScopeGap: 4
    // The sidebar: the line between places, tree and drives, and the tree,
    // its rows the height of a place, each level this far in, with its
    // chevron (turning as it opens) and folder icon. New rows slide in from
    // just above, one after another, this much apart.
    readonly property color filesRule: glassRim
    readonly property int filesRuleInset: 6
    readonly property int filesTreeIndent: 14
    readonly property int filesTreeChevron: 14
    readonly property int filesTreeGap: 4
    readonly property int filesTreeRise: 4
    readonly property int filesTreeStagger: 30
    // The Home page: Quick access and the drives as tiles in rows, a big
    // icon beside a name and a fainter line (where the folder is, a drive's
    // free space under its bar), then Recent as a list with its own
    // columns after the name (Date used, Location). The sections sit this
    // far in and apart, under their headings.
    readonly property int filesHomePad: 20
    readonly property int filesHomeSectionGap: 18
    readonly property int filesHomeTileWidth: 200
    readonly property int filesHomeTileHeight: 58
    readonly property int filesHomeTileIconSize: 26
    readonly property list<int> filesRecentColumns: [130, 220]
    // The most of the sidebar's height its places and drives take before
    // they scroll, so the folder tree keeps room under many pins.
    readonly property real filesPlacesMaxShare: 0.5

    // Copies and moves: each running job's card stacked at the window's
    // bottom right, and the name clash dialog in its middle, its two sides
    // with a big icon each and its answers as round buttons.
    readonly property int filesJobWidth: 290
    readonly property int filesOverlayInset: 16
    readonly property int filesClashWidth: 440
    readonly property int filesClashIconSize: 40
    readonly property int filesClashButtonHeight: 34
    // The scale the dialog pops in from and shrinks back to.
    readonly property real filesClashEnterScale: 0.97
    // Tabs along the window's top, as in Windows 11's Explorer: each shares
    // the room between these widths, the shown one rounded on top and raised
    // into the navigation row under it. A tab dragged this far below the
    // row comes out as a window of its own.
    readonly property int filesTabHeight: 32
    readonly property int filesTabPadTop: 6
    readonly property int filesTabGap: 2
    readonly property int filesTabMinWidth: 96
    readonly property int filesTabMaxWidth: 220
    readonly property int filesTabRadius: menuRowRadius
    readonly property int filesTabCloseSize: 20
    readonly property int filesTabTearDistance: 40
    // Right-click menus: wide enough for a label and its shortcut, rows a
    // little shorter than an app menu's, an item's submenu (Open with)
    // beside it. Cut items read faded until they're pasted.
    readonly property int filesMenuWidth: 300
    readonly property int filesMenuRowHeight: 28
    readonly property int filesMenuSeparatorGap: 3
    readonly property real filesCutOpacity: 0.5
    // The details pane on the right of the file list: its big icon, the
    // lines between its parts, and its rows of who may do what.
    readonly property int filesDetailsWidth: 280
    readonly property int filesDetailsPad: 16
    readonly property int filesDetailsIconSize: 56
    readonly property int filesDetailsRowHeight: 42
    // Drag and drop: how far a press moves before it drags, the tile under
    // the pointer (the item's icon, and the count in an accent bubble at its
    // corner for more than one), the wash on the folder a drop would go
    // into, and how long the pointer rests on a folder before it opens.
    readonly property int filesDragStart: 6
    readonly property int filesDragTile: 52
    readonly property int filesDragIcon: 28
    readonly property int filesDragCount: 20
    readonly property color filesDropTarget: alpha(primary, 0.24)
    readonly property int durationFilesSpring: 800
    // The Properties dialog (Alt+Enter): this wide, its labels in a column
    // this wide, its rows this tall, the drive's fill bar this thick, and
    // the access grid's check columns this wide. A check box that can't be
    // changed fades to this.
    readonly property int filesPropertiesWidth: 440
    readonly property int filesPropertiesLabelWidth: 110
    readonly property int filesPropertiesRowHeight: 24
    readonly property int filesPropertiesBarHeight: 6
    readonly property int filesPropertiesCheckColumn: 64
    readonly property real filesCheckDisabledOpacity: 0.45
    // The preview pane (Alt+P) at the file list's right: this wide, its
    // picture or text this far in (pictures are decoded at
    // Protocol.FILES_PREVIEW_PICTURE_PX, its width at twice the scale, the
    // same for every window size), and the big icon and name (in at most this many lines) for a
    // file it can't show. It previews a selection that has rested this
    // long, so arrowing through a folder never asks for every file on the
    // way, and shows a spinner once an answer has been this long in coming.
    readonly property int filesPreviewWidth: 300
    readonly property int filesPreviewPad: 16
    readonly property int filesPreviewIconSize: 64
    readonly property int filesPreviewNameLines: 3
    readonly property int filesPreviewDelay: 120
    readonly property int filesPreviewSlow: 400
    // A Nova Files window as an app's Open or Save dialog: the bar under
    // the list with the name field, the file type button (its menu opens
    // upward) and the answers.
    readonly property int filesPickerFilterWidth: 220

    // Type
    readonly property string fontSans: "Inter"
    readonly property string fontMono: "JetBrains Mono"
    readonly property string fontDisplay: "Instrument Serif"
    // Weights: running text, labels and names, titles and picked options,
    // and the unread badge's count.
    readonly property int weightBody: Font.Normal
    readonly property int weightLabel: Font.Medium
    readonly property int weightTitle: Font.DemiBold
    readonly property int weightBadge: Font.Bold
    // The type scale: six steps, from labels up, then two big numbers.
    // Small caps labels and badges.
    readonly property int fontSizeMicro: 10
    // Secondary text: notification bodies, subtitles, times.
    readonly property int fontSizeSmall: 12
    // Body text: rows, names, the central pill's time.
    readonly property int fontSize: 13
    // A page's title beside its back button, and the search field.
    readonly property int fontSizeHeading: 15
    // A name or number on its own: the lock screen, a pairing code, traffic.
    readonly property int fontSizeTitle: 18
    // Big words: the quick settings greeting (it shrinks to fontSizeHeading
    // to fit), an empty list.
    readonly property int fontSizeDisplay: 24
    // A panel's headline number.
    readonly property int fontSizeHero: 56
    // The capture countdown, across the screen.
    readonly property int fontSizeCountdown: 96
    readonly property int lineHeightHero: 60
    // The headline's tracking, as a share of its size (tighter when big), and
    // the size of a unit (like %) beside it, relative to its digits.
    readonly property real trackingHero: -0.02
    readonly property real heroUnitScale: 0.42
    // Line height for text that wraps, like a hint under a meter.
    readonly property real lineHeightWrapped: 1.5

    // One notch of a mouse wheel, in Qt's angleDelta units (eighths of a
    // degree); a touchpad's small deltas add up to it.
    readonly property int wheelNotch: 120

    // Motion (ms)
    readonly property int durationFast: 120
    readonly property int durationPanel: 250
    // Content fading in once a panel has grown, and meters filling.
    readonly property int durationFade: 140
    readonly property int durationFill: 600
    readonly property int durationColor: 200
    // When MorphReveal starts something, as a share of the panel's morph:
    // a panel's content, meters and rings filling, and the right pill's
    // overflow count, which waits for the pill to settle.
    readonly property real revealContent: 0.5
    readonly property real revealFill: 0.6
    readonly property real revealPillCount: 0.8
    // The AI usage panel's parts fade in one after another: its title at
    // this share of the morph, then the plan and first meter, the second
    // meter and the footer, each this much later.
    readonly property real usageRevealFirst: 0.4
    readonly property real usageRevealStagger: 0.15
    // Something changing size as its content changes, like the launcher
    // as results come and go.
    readonly property int durationResize: 180
    // One turn of a loading spinner.
    readonly property int durationSpin: 1000
    // A wrong password's shake, and the lock screen fading before it unlocks.
    readonly property int durationShake: 360
    readonly property int durationUnlock: 400
    // The scale something new grows in from.
    readonly property real enterScale: 0.6
    // How long an action that ends the session waits for its second click.
    readonly property int durationConfirm: 4000
    // How long the bar stays after the pointer leaves it while it's only
    // brought back over a fullscreen window, so moving from the top edge
    // down to a pill doesn't hide it again.
    readonly property int durationPeekLinger: 400
    // The wallpaper carousel moving one card, and the preview fading to
    // the next image.
    readonly property int durationCarouselStep: 280
    readonly property int durationPreview: 250
    // After a pick, how long the preview stays while awww fades the new
    // image in under it: its fade (novad's TRANSITION_SECONDS) and a little.
    readonly property int durationWallpaperHandOver: 1100
    // The cubic-bezier every panel motion uses (fast out, gentle stop).
    readonly property list<real> easing: [0.2, 0.8, 0.2, 1, 1, 1]

    // `from` blended into `to` by `amount` (0 is `from`, 1 is `to`), alpha
    // too, so glass can turn solid.
    function mix(from: color, to: color, amount: real): color {
        const lerp = (a, b) => a + (b - a) * amount;
        return Qt.rgba(lerp(from.r, to.r), lerp(from.g, to.g), lerp(from.b, to.b), lerp(from.a, to.a));
    }

    // `color` at `opacity` (0..1), whatever its own alpha.
    function alpha(color: color, opacity: real): color {
        return Qt.rgba(color.r, color.g, color.b, opacity);
    }

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
