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
import "../daemon"
import "../daemon/protocol.js" as Protocol
import "../slots/slots.js" as Slots
import "../slots"
import "quick_settings.js" as QuickSettings
import "../util/numbers.js" as Numbers
import "../icons"

// The quick settings drawer: glass against the right edge of the screen,
// top to bottom, opened by the right pill's button or `nova toggle
// quick-settings`. It grows out of the right pill (StagePanel `from`: the
// pill hides once it's drawn over), keeps the pill's buttons where they
// were, and its sections fade in one after another. A click outside
// closes it.
//
// What's in it comes from feature manifests: the first page (MainPage)
// hosts the quick-settings.tiles, .sliders, .cards, .fill and .foot slots,
// and every quick-settings.pages slot is a page that slides in from the
// right and back out to the first page. Each slot's view is a thin adapter
// (slots/) that reads its feature's service and gets `qs` (QsContext) for
// what the sidebar shares; the views it draws only take properties, so
// ui/tests can drive them.
StagePanel {
    id: panel

    required property StageScreen output
    // The right pill on the stage, and where it is in screen coordinates:
    // it grows out of it, and its buttons stay where the pill had them.
    required property Item pillItem
    property rect pill: Qt.rect(output.width - Tokens.barMargin, Tokens.barGap, 0, Tokens.barHeight)
    from: pillItem
    // The screen's stage (Stage.qml), which the pill's buttons open their
    // panels through, here as on the pill.
    required property var stage

    name: Protocol.FEATURE_QUICK_SETTINGS
    placement: "right"

    // The page it shows: QuickSettings.PAGE_MAIN, or a page's view id.
    property string page: QuickSettings.PAGE_MAIN

    // What features put in it, from their manifests: each host's slots
    // (slots.js forHost) that have a view. Hosts keep their views in step
    // with these (Slots.sync), so a view that stays isn't loaded again.
    function slotsOf(host: string): var {
        return Slots.forHost(Daemon.features, host).filter(slot => Views.url(slot.view).toString() !== "");
    }
    readonly property var tileSlots: slotsOf(Slots.HOST_QUICK_SETTINGS_TILES)
    readonly property var sliderSlots: slotsOf(Slots.HOST_QUICK_SETTINGS_SLIDERS)
    readonly property var cardSlots: slotsOf(Slots.HOST_QUICK_SETTINGS_CARDS)
    readonly property var fillSlots: slotsOf(Slots.HOST_QUICK_SETTINGS_FILL)
    readonly property var footSlots: slotsOf(Slots.HOST_QUICK_SETTINGS_FOOT)
    readonly property var pageSlots: slotsOf(Slots.HOST_QUICK_SETTINGS_PAGES)
    onPageSlotsChanged: {
        Slots.sync(pageModel, pageSlots);
        // Its page went with its feature: back to the first.
        if (!pageSlots.some(slot => slot.view === page))
            page = QuickSettings.PAGE_MAIN;
    }
    Component.onCompleted: Slots.sync(pageModel, pageSlots)
    ListModel {
        id: pageModel
    }

    // What every slot gets: where the sidebar is, and its requests.
    QsContext {
        id: context
        open: panel.open
        page: panel.page
        fill: panel.fill
        now: panel.now
        outputName: panel.output.name
        headerInset: pages.headerInset
        menuArea: pages
        onPageRequested: page => panel.page = page
        onCloseRequested: panel.open = false
        onTrayMenuRequested: (entry, at) => trayMenu.show(entry, at)
    }

    readonly property Morph morph: Morph {
        open: panel.open
    }
    // The rings fill once it's open, and empty with it as it closes.
    property real fill: open ? 1 : 0
    MorphReveal on fill {
        id: fillReveal
        delay: Tokens.revealFill
        duration: Tokens.durationFill
        eased: true
    }
    moving: morph.moving || fillReveal.running || ((mainPage.item as MainPage)?.revealing ?? false) || pages.sliding

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
        if (!open)
            trayMenu.entry = null;
    }
    // Back to the first page once it has closed, so it opens there.
    onMovingChanged: {
        if (!moving && !open)
            page = QuickSettings.PAGE_MAIN;
    }

    // The header's uptime, a minute at a time while it's open, on the
    // minute.
    SystemClock {
        id: clock
        enabled: panel.open
        precision: SystemClock.Minutes
    }
    readonly property date now: clock.date
    // Gone while open (its screen went away). Slots undo their own watching.
    Component.onDestruction: VolumeService.showElsewhere(output.name, false)
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

        readonly property bool sliding: {
            // itemAt doesn't notify: reading the revision re-evaluates this
            // for new delegates.
            pageRepeater.revision;
            let any = mainSlot.sliding;
            for (let i = 0; i < pageRepeater.count; i++)
                any = any || ((pageRepeater.itemAt(i) as PageSlot)?.sliding ?? false);
            return any;
        }

        PageSlot {
            id: mainSlot
            key: QuickSettings.PAGE_MAIN

            // Only while it's open or moving: closed, it keeps no tiles,
            // tray or notification list on any screen (IMP-157).
            Loader {
                id: mainPage
                // Not `moving`: that follows the page's own reveal.
                active: panel.open || panel.morph.moving || fillReveal.running
                sourceComponent: MainPage {
                    width: pages.width
                    height: pages.height
                    headerInset: pages.headerInset
                    // From false once loaded, so its sections fade in one after
                    // another as it opens.
                    shown: panel.open && mainPage.status === Loader.Ready
                    qs: context
                    logo: QuickSettings.logo(QuickSettingsService.host?.os, Qt.resolvedUrl("logos"), path => AppIcons.source(path))
                    host: QuickSettingsService.host
                    viewUrl: view => Views.url(view)
                    tiles: panel.tileSlots
                    sliders: panel.sliderSlots
                    cards: panel.cardSlots
                    fill: panel.fillSlots[0]?.view ?? ""
                    foot: panel.footSlots[0]?.view ?? ""
                }
            }
        }

        // The other pages, from quick-settings.pages slots, each keyed by its
        // view id and loaded only while it shows or slides.
        Repeater {
            id: pageRepeater
            model: pageModel
            property int revision: 0
            onItemAdded: revision++

            PageSlot {
                id: pageSlot
                required property string view
                key: view

                Loader {
                    readonly property bool live: pageSlot.live
                    anchors.fill: parent
                    onLiveChanged: load()
                    Component.onCompleted: load()
                    function load() {
                        if (live)
                            setSource(Views.url(pageSlot.key), {
                                qs: context
                            });
                        else
                            setSource("");
                    }
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
        x: pages.x + Numbers.clamp(at.x - width / 2, 0, pages.width - width)
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
            runEntry: run => TrayService.menuPicked(trayMenu.entry, run)
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
        stage: panel.stage
        slots: Slots.forHost(Daemon.features, Slots.HOST_RIGHT_PILL_BUTTONS)
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
