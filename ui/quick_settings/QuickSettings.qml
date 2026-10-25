pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../bar"
import "../stage"
import "../theme"
import "../volume/service"
import "service"
import "../daemon/protocol.js" as Protocol
import "quick_settings.js" as QuickSettings

// The quick settings drawer: glass against the right edge of the screen,
// top to bottom, opened by the right pill's button or `nova toggle
// quick-settings`. It grows out of the right pill (StagePanel `from`: the
// pill hides once it's drawn over), keeps the pill's buttons where they
// were, and its sections fade in one after another. A click outside
// closes it.
//
// Pages slide in from the right (sound, resources) and back out to the
// first page. Reading and acting go through QuickSettingsService,
// VolumeService and SoundService; the pages themselves only get
// properties, so ui/tests can drive them.
StagePanel {
    id: panel

    required property ShellScreen screen
    // The right pill on the stage, and where it is in screen coordinates:
    // it grows out of it, and its buttons stay where the pill had them.
    required property Item pillItem
    property rect pill: Qt.rect(screen.width - Tokens.barMargin, Tokens.barMargin, 0, Tokens.barHeight)
    from: pillItem
    // The pill's buttons, as on the pill.
    property bool power: false
    signal quickSettingsClicked
    signal powerClicked

    name: Protocol.FEATURE_QUICK_SETTINGS
    placement: "right"

    // "main", "sound" or "resources".
    property string page: "main"

    readonly property Morph morph: Morph {
        open: panel.open
    }
    // The rings fill once it's open, and empty with it as it closes.
    property real fill: open ? 1 : 0
    MorphReveal on fill {
        id: fillReveal
        delay: 0.6
        duration: Tokens.durationFill
        eased: true
    }
    moving: morph.moving || fillReveal.running || main.revealing || pages.sliding

    // Open, where it sits: against the right edge, top to bottom, its right
    // corners past the edge so only the left ones are round. Closed, the
    // pill.
    readonly property rect full: Qt.rect(screen.width - Tokens.quickSettingsWidth, 0, Tokens.quickSettingsWidth + Tokens.radiusPanel, screen.height)
    x: morph.lerp(pill.x, full.x)
    y: morph.lerp(pill.y, full.y)
    width: morph.lerp(pill.width, full.width)
    height: morph.lerp(pill.height, full.height)
    radius: morph.lerp(pill.height / 2, Tokens.radiusPanel)
    openHeight: full.height
    area: Qt.rect(full.x, 0, screen.width - full.x, screen.height)
    clip: true

    onOpenChanged: {
        console.info(`nova: quick settings ${open ? "opened" : "closed"}`);
        QuickSettingsService.watching = open;
        VolumeService.showElsewhere(screen.name, open);
        if (open)
            now = new Date();
    }
    // Back to the first page once it has closed, so it opens there.
    onMovingChanged: {
        if (!moving && !open)
            page = "main";
    }

    readonly property string outputName: VolumeService.sink ? (VolumeService.sink.nickname || VolumeService.sink.description || VolumeService.sink.name) : ""

    // The header's uptime, a minute at a time while it's open.
    property date now: new Date()
    Component.onDestruction: VolumeService.showElsewhere(screen.name, false)
    Timer {
        interval: 60 * 1000
        running: panel.open
        repeat: true
        onTriggered: panel.now = new Date()
    }

    Glass {
        anchors.fill: parent
        radius: panel.radius
    }

    // Where the pages sit once it's open, their top row level with the
    // pill's buttons; it grows over them.
    Item {
        id: pages
        x: panel.full.x - panel.x + Tokens.quickSettingsPad
        y: panel.pill.y - panel.y
        width: Tokens.quickSettingsWidth - 2 * Tokens.quickSettingsPad
        height: panel.full.height - panel.pill.y - Tokens.quickSettingsPad
        // What the top row leaves for the pill's buttons.
        readonly property real headerInset: panel.screen.width - Tokens.quickSettingsPad - panel.pill.x + Tokens.gap

        readonly property bool sliding: [mainSlot, soundSlot, resourcesSlot].some(slot => slot.sliding)

        PageSlot {
            id: mainSlot
            key: "main"

            MainPage {
                id: main
                width: pages.width
                height: pages.height
                headerInset: pages.headerInset
                shown: panel.open
                fill: panel.fill
                logo: QuickSettings.logo(QuickSettingsService.host?.os, Qt.resolvedUrl("logos"), name => Quickshell.iconPath(name, true))
                host: QuickSettingsService.host
                now: panel.now
                keepAwake: QuickSettingsService.keepAwake
                micAvailable: SoundService.sourceReady
                micMuted: SoundService.sourceMuted
                outputAvailable: VolumeService.ready
                outputName: panel.outputName
                outputKind: VolumeService.output
                volume: VolumeService.volume
                muted: VolumeService.muted
                recorders: SoundService.recorders
                micVolume: SoundService.sourceVolume
                reading: QuickSettingsService.resources
                onKeepAwakeToggled: QuickSettingsService.keepAwake = !QuickSettingsService.keepAwake
                onMicMuteToggled: SoundService.toggleSourceMute()
                onVolumeMoved: value => VolumeService.slide(value)
                onMuteToggled: VolumeService.toggleMute()
                onMicVolumeMoved: value => SoundService.setSourceVolume(value)
                onPageRequested: page => panel.page = page
            }
        }

        PageSlot {
            id: soundSlot
            key: "sound"

            Loader {
                active: soundSlot.live
                onStatusChanged: SoundService.listing = status === Loader.Ready
                sourceComponent: SoundPage {
                    width: pages.width
                    height: pages.height
                    headerInset: pages.headerInset
                    outputAvailable: VolumeService.ready
                    outputName: panel.outputName
                    outputKind: VolumeService.output
                    volume: VolumeService.volume
                    muted: VolumeService.muted
                    inputAvailable: SoundService.sourceReady
                    inputName: SoundService.sourceName
                    inputVolume: SoundService.sourceVolume
                    inputMuted: SoundService.sourceMuted
                    onInputVolumeMoved: value => SoundService.setSourceVolume(value)
                    onInputMuteToggled: SoundService.toggleSourceMute()
                    outputs: SoundService.outputs
                    inputs: SoundService.inputs
                    apps: SoundService.apps
                    onBack: panel.page = "main"
                    onVolumeMoved: value => VolumeService.slide(value)
                    onMuteToggled: VolumeService.toggleMute()
                    onOutputPicked: id => SoundService.useOutput(id)
                    onInputPicked: id => SoundService.useInput(id)
                    onAppVolumeMoved: (ids, value) => SoundService.setVolumes(ids, value)
                    onAppMuteToggled: (ids, muted) => SoundService.setMuted(ids, muted)
                    // A desktop entry's icon may be a file instead of a name.
                    iconSource: name => name === "" ? "" : name.startsWith("/") ? `file://${name}` : Quickshell.iconPath(name, true)
                }
            }
        }

        PageSlot {
            id: resourcesSlot
            key: "resources"

            Loader {
                active: resourcesSlot.live
                sourceComponent: ResourcesPage {
                    width: pages.width
                    height: pages.height
                    headerInset: pages.headerInset
                    reading: QuickSettingsService.resources
                    onBack: panel.page = "main"
                    onEndRequested: key => QuickSettingsService.endApp(key)
                }
            }
        }
    }

    // The pill's buttons where the pill has them: it opened over the pill,
    // and the button that opened it closes it.
    PillButtons {
        x: panel.pill.x - panel.x + (panel.pill.width - width) / 2
        y: panel.pill.y - panel.y + (panel.pill.height - height) / 2
        quickSettings: true
        power: panel.power
        quickSettingsOpen: panel.open
        onQuickSettingsClicked: panel.quickSettingsClicked()
        onPowerClicked: panel.powerClicked()
    }

    // A page: in place while it's the one shown, off to its side (the first
    // page to the left, the others to the right) otherwise, sliding and
    // fading between. Pages other than the first load only while `live`.
    component PageSlot: Item {
        id: slot

        required property string key
        readonly property bool current: panel.page === key
        readonly property bool sliding: slide.running || fade.running
        readonly property bool live: current || sliding

        width: pages.width
        height: pages.height
        x: current ? 0 : key === "main" ? -Tokens.pageSlide : Tokens.pageSlide
        opacity: current ? 1 : 0
        visible: opacity > 0

        Behavior on x {
            enabled: panel.open
            NumberAnimation {
                id: slide
                duration: Tokens.durationPage
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }
        Behavior on opacity {
            enabled: panel.open
            NumberAnimation {
                id: fade
                duration: Tokens.durationPage
            }
        }
    }
}
