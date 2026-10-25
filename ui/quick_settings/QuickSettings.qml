pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../stage"
import "../theme"
import "../volume/service"
import "service"
import "../daemon/protocol.js" as Protocol
import "quick_settings.js" as QuickSettings

// The quick settings sidebar: full-height glass on the right, from below
// the bar to the screen's bottom margin, opened by the right pill's button
// or `nova toggle quick-settings`. It grows out of the right pill (fading
// in, since it's drawn over it) and its sections fade in one after
// another. A click outside closes it.
//
// Pages slide in from the right (sound, resources) and back out to the
// first page. Reading and acting go through QuickSettingsService,
// VolumeService and SoundService; the pages themselves only get
// properties, so ui/tests can drive them.
StagePanel {
    id: panel

    required property ShellScreen screen
    // The right pill on the stage, in screen coordinates: where it grows
    // from.
    property rect pill: Qt.rect(screen.width - Tokens.barMargin, Tokens.barMargin, 0, Tokens.barHeight)

    name: Protocol.FEATURE_QUICK_SETTINGS
    placement: "right"
    closesOnClickOutside: true

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

    // Open, where it sits; closed, the pill.
    readonly property rect full: {
        const top = Tokens.barMargin + Tokens.barHeight + Tokens.barMargin;
        return Qt.rect(screen.width - Tokens.barMargin - Tokens.quickSettingsWidth, top, Tokens.quickSettingsWidth, screen.height - top - Tokens.barMargin);
    }
    x: morph.lerp(pill.x, full.x)
    y: morph.lerp(pill.y, full.y)
    width: morph.lerp(pill.width, full.width)
    height: morph.lerp(pill.height, full.height)
    radius: morph.lerp(pill.height / 2, Tokens.radiusPanel)
    openHeight: full.height
    area: Qt.rect(Math.min(pill.x, full.x), pill.y, Math.max(pill.x + pill.width, full.x + full.width) - Math.min(pill.x, full.x), full.y + full.height - pill.y)
    // It's drawn over the pill, so it fades in as it leaves it.
    opacity: Math.min(1, morph.t / Tokens.panelFadeShare)
    clip: true

    onOpenChanged: {
        console.info(`nova: quick settings ${open ? "opened" : "closed"}`);
        QuickSettingsService.watching = open;
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

    // Where the pages sit once it's open; it grows over them.
    Item {
        id: pages
        x: panel.full.x - panel.x + Tokens.quickSettingsPad
        y: panel.full.y - panel.y + Tokens.quickSettingsPad
        width: panel.full.width - 2 * Tokens.quickSettingsPad
        height: panel.full.height - 2 * Tokens.quickSettingsPad

        readonly property bool sliding: [mainSlot, soundSlot, resourcesSlot].some(slot => slot.sliding)

        PageSlot {
            id: mainSlot
            key: "main"

            MainPage {
                id: main
                width: pages.width
                height: pages.height
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
                onVolumeMoved: value => VolumeService.set(value)
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
                    outputAvailable: VolumeService.ready
                    outputName: panel.outputName
                    outputKind: VolumeService.output
                    volume: VolumeService.volume
                    muted: VolumeService.muted
                    outputs: SoundService.outputs
                    inputs: SoundService.inputs
                    apps: SoundService.apps
                    onBack: panel.page = "main"
                    onVolumeMoved: value => VolumeService.set(value)
                    onMuteToggled: VolumeService.toggleMute()
                    onOutputPicked: id => SoundService.useOutput(id)
                    onInputPicked: id => SoundService.useInput(id)
                    onAppVolumeMoved: (id, value) => SoundService.setVolume(id, value)
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
                    reading: QuickSettingsService.resources
                    onBack: panel.page = "main"
                }
            }
        }
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
