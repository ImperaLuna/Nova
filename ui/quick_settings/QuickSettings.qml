pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import "../bar"
import "../notifications/service"
import "../stage"
import "../theme"
import "../volume/service"
import "service"
import "../tray"
import "../tray/service"
import "../daemon/protocol.js" as Protocol
import "quick_settings.js" as QuickSettings

// The quick settings drawer: glass against the right edge of the screen,
// top to bottom, opened by the right pill's button or `nova toggle
// quick-settings`. It grows out of the right pill (StagePanel `from`: the
// pill hides once it's drawn over), keeps the pill's buttons where they
// were, and its sections fade in one after another. A click outside
// closes it.
//
// Pages slide in from the right (sound, resources, notification settings)
// and back out to the first page. Reading and acting go through
// QuickSettingsService, VolumeService, SoundService and
// NotificationsService; the pages themselves only get
// properties, so ui/tests can drive them.
StagePanel {
    id: panel

    required property StageScreen output
    // The right pill on the stage, and where it is in screen coordinates:
    // it grows out of it, and its buttons stay where the pill had them.
    required property Item pillItem
    property rect pill: Qt.rect(output.width - Tokens.barMargin, Tokens.barGap, 0, Tokens.barHeight)
    from: pillItem
    // The pill's buttons, as on the pill.
    property bool power: false
    signal quickSettingsClicked
    signal powerClicked

    name: Protocol.FEATURE_QUICK_SETTINGS
    placement: "right"

    // "main", "sound", "resources" or "notifications".
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
    readonly property rect full: Qt.rect(output.width - Tokens.quickSettingsWidth, 0, Tokens.quickSettingsWidth + Tokens.radiusPanel, output.height)
    x: morph.lerp(pill.x, full.x)
    y: morph.lerp(pill.y, full.y)
    width: morph.lerp(pill.width, full.width)
    height: morph.lerp(pill.height, full.height)
    radius: morph.lerp(pill.height / 2, Tokens.radiusPanel)
    openHeight: full.height
    area: Qt.rect(full.x, 0, output.width - full.x, output.height)
    clip: true

    onOpenChanged: {
        console.info(`nova: quick settings ${open ? "opened" : "closed"}`);
        VolumeService.showElsewhere(output.name, open);
        if (open)
            now = new Date();
        else
            trayMenu.entry = null;
    }
    // Resources show on the first page (its card) and their own page: novad
    // reads them only while one of those is open.
    readonly property bool resourcesShown: open && (page === "main" || page === "resources")
    onResourcesShownChanged: QuickSettingsService.watching = resourcesShown
    // The list is on the first page: another page hides it.
    readonly property bool listShown: open && page === "main"
    onListShownChanged: NotificationsService.showList(output.name, listShown)
    // Back to the first page once it has closed, so it opens there.
    onMovingChanged: {
        if (!moving && !open)
            page = "main";
    }

    readonly property string outputName: VolumeService.sink ? (VolumeService.sink.nickname || VolumeService.sink.description || VolumeService.sink.name) : ""

    // The header's uptime, a minute at a time while it's open.
    property date now: new Date()
    // Gone while open (its screen went away): nothing reads resources now.
    Component.onDestruction: {
        VolumeService.showElsewhere(output.name, false);
        NotificationsService.showList(output.name, false);
        if (resourcesShown)
            QuickSettingsService.watching = false;
    }
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
        readonly property real headerInset: panel.output.width - Tokens.quickSettingsPad - (panel.buttonsX - Tokens.pillButtonInset) + Tokens.gap

        readonly property bool sliding: [mainSlot, soundSlot, resourcesSlot, notificationsSlot].some(slot => slot.sliding)

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
                trayApps: TrayService.all
                trayIconSource: entry => TrayService.iconSource(entry)
                onTrayActivated: entry => {
                    if (TrayService.opensMenu(entry)) {
                        trayMenu.show(entry, null);
                    } else {
                        entry.item.activate();
                        panel.open = false;
                    }
                }
                onTrayMenuRequested: (entry, at) => trayMenu.show(entry, at)
                onTraySecondaryActivated: entry => entry.item.secondaryActivate()
                notificationsShown: NotificationsService.ready
                notifications: NotificationsService.items
                doNotDisturb: NotificationsService.doNotDisturb
                iconSource: name => NotificationsService.iconSource(name)
                // Opening one takes you to its app.
                onNotificationOpened: notification => {
                    NotificationsService.open(notification, panel.output.name);
                    panel.open = false;
                }
                onNotificationDismissed: id => NotificationsService.dismiss(id)
                onNotificationsCleared: app => NotificationsService.clear(app)
                onNotificationAppMuted: app => NotificationsService.mute(app, true)
                onDoNotDisturbToggled: NotificationsService.setDoNotDisturb(!NotificationsService.doNotDisturb)
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

        PageSlot {
            id: notificationsSlot
            key: "notifications"

            Loader {
                active: notificationsSlot.live
                sourceComponent: NotificationSettingsPage {
                    width: pages.width
                    height: pages.height
                    headerInset: pages.headerInset
                    doNotDisturb: NotificationsService.doNotDisturb
                    apps: NotificationsService.apps
                    muted: NotificationsService.muted
                    iconSource: name => NotificationsService.iconSource(name)
                    onBack: panel.page = "main"
                    onDoNotDisturbToggled: NotificationsService.setDoNotDisturb(!NotificationsService.doNotDisturb)
                    onMuteToggled: (app, muted) => NotificationsService.mute(app, muted)
                }
            }
        }
    }

    // A tray app's menu over the Tray row: a click beside it closes it, an
    // entry that ran closes quick settings too (hiding or showing an app
    // leaves it open, to see the change).
    MouseArea {
        anchors.fill: pages
        visible: trayMenu.entry !== null
        onClicked: trayMenu.entry = null
    }
    Rectangle {
        id: trayMenu

        property var entry: null
        property point at: Qt.point(0, 0)
        property bool keepOpen: false

        // Under the icon at `at` (in the main page), or the row's middle.
        function show(entry: var, at: var) {
            this.at = at ?? Qt.point(pages.width / 2, pages.height / 2);
            keepOpen = false;
            this.entry = entry;
        }

        visible: entry !== null && panel.open
        x: pages.x + Math.max(0, Math.min(pages.width - width, at.x - width / 2))
        y: pages.y + Math.min(pages.height - height, at.y + Tokens.trayGridCell / 2 + Tokens.menuGap)
        width: list.width
        height: list.implicitHeight
        radius: Tokens.radiusCard
        color: Tokens.surface
        border.width: Tokens.dividerWidth
        border.color: Tokens.outline

        TrayMenuOpener {
            id: opener
            entry: trayMenu.entry
        }

        TrayMenuList {
            id: list
            entry: trayMenu.entry
            entries: opener.entries
            inSubmenu: opener.inSubmenu
            onSubmenuRequested: submenu => opener.open(submenu)
            onBackRequested: opener.back()
            // Within the panel; longer menus scroll.
            maxHeight: pages.height
            iconSource: trayMenu.entry ? TrayService.iconSource(trayMenu.entry) : ""
            onHideRequested: (key, hidden) => {
                trayMenu.keepOpen = true;
                TrayService.hide(key, hidden);
            }
            onDone: {
                trayMenu.entry = null;
                if (!trayMenu.keepOpen)
                    panel.open = false;
            }
        }
    }
    // Where the pill has its buttons, in screen coordinates: at its right
    // end, after the tray's chevron.
    readonly property real buttonsX: pill.x + pill.width - Tokens.pillButtonInset - buttons.width

    // The pill's buttons where the pill has them: it opened over the pill,
    // and the button that opened it closes it.
    PillButtons {
        id: buttons
        x: panel.buttonsX - panel.x
        y: panel.pill.y - panel.y + (panel.pill.height - height) / 2
        quickSettings: true
        power: panel.power
        quickSettingsOpen: panel.open
        unread: NotificationsService.ready ? NotificationsService.unread : 0
        doNotDisturb: NotificationsService.ready && NotificationsService.doNotDisturb
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
