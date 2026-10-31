pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../volume/volume.js" as Volume
import "quick_settings.js" as QuickSettings
import "network.js" as Network
import "bluetooth.js" as Bluetooth
import "../text/duration.js" as Duration
import "../tray"

// The sidebar's first page: the header, the toggle tiles, the sliders, the
// notifications in the space below them (with the notifications feature
// on), and the resources card pinned at the foot. Everything comes in as
// properties and goes out as signals, so ui/tests can drive it without
// PipeWire or novad.
Item {
    id: page

    // The sidebar is open: sections fade in one after another.
    property bool shown: true
    // How far the resources rings are filled in (they fill once it's open).
    property real fill: 1

    // Header.
    property string logo: ""
    property var host: null
    property date now: new Date()

    // The network tile, while its feature is on: a network event's data.
    property bool networkShown: false
    property var network: null

    // Keep awake and the microphone.
    property bool keepAwake: false
    property bool micAvailable: false
    property bool micMuted: false
    // Bluetooth, while its feature is on: a bluetooth event's data, or null.
    property bool bluetoothShown: false
    property var bluetooth: null

    // The default output: whether there is one, its name and kind
    // (volume.js outputKind), volume and mute.
    property bool outputAvailable: false
    property string outputName: ""
    property string outputKind: "none"
    property real volume: 0
    property bool muted: false
    // Apps recording from the mic now; the mic slider shows only then.
    property list<string> recorders: []
    property real micVolume: 0

    property var reading: null
    // The wireless devices connected now (a peripherals event's list).
    property var devices: []
    // Every tray app, hidden ones too (TrayService.all), and what an Image
    // shows for one.
    property var trayApps: []
    property var trayIconSource: entry => ""

    // Notifications (NotificationList), while their feature is on.
    property bool notificationsShown: false
    property var notifications: []
    property bool doNotDisturb: false
    property var iconSource: name => ""
    // Room the top row leaves on its right (the pill's buttons).
    property real headerInset: 0

    signal keepAwakeToggled
    signal micMuteToggled
    // Turn Bluetooth on (true) or off.
    signal bluetoothPowered(bool on)
    signal volumeMoved(real value)
    signal muteToggled
    signal micVolumeMoved(real value)
    signal pageRequested(string page)
    // A tray app's icon was clicked; `at` is its middle, in this page.
    signal trayActivated(var entry)
    signal trayMenuRequested(var entry, point at)
    signal traySecondaryActivated(var entry)

    signal notificationOpened(var notification)
    signal notificationDismissed(int id)
    signal notificationsCleared(var app)
    signal notificationAppMuted(string app)
    signal doNotDisturbToggled

    readonly property bool revealing: [header, tiles, sliders, deviceSection, tray, notes, foot].some(section => section.running) || deviceCard.running

    Column {
        id: sections
        width: page.width
        spacing: Tokens.quickSettingsGap

        Reveal {
            id: header
            width: parent.width
            shown: page.shown
            order: 0

            Header {
                width: parent.width - page.headerInset
                logo: page.logo
                greeting: QuickSettings.greeting(page.now.getHours())
                name: page.host?.user ?? ""
                detail: page.host ? page.host.host + " · " + Duration.uptime(page.now.getTime() / 1000 - page.host.booted_at) : ""
            }
        }

        Reveal {
            id: tiles
            width: parent.width
            shown: page.shown
            order: 1

            Grid {
                id: grid
                width: parent.width
                columns: 2
                spacing: Tokens.gap
                readonly property real cell: (width - spacing) / columns

                // Opens the network page, the whole tile: its switches are
                // there.
                Tile {
                    objectName: "networkTile"
                    visible: page.networkShown
                    width: grid.cell
                    icon: Network.tileIcon(page.network)
                    iconFill: Network.iconFill(icon)
                    title: qsTr("Network")
                    subtitle: Network.tileSubtitle(page.network)
                    on: (page.network?.primary ?? "") !== "" || Network.connecting(page.network) !== ""
                    opens: true
                    onToggled: page.pageRequested("network")
                    onOpened: page.pageRequested("network")
                }
                // A click turns it on when it's off, and opens the page when
                // it's on: turning it off is the page's switch, so a click
                // meant for the page doesn't drop the headphones. Turned off
                // by a hardware switch, it opens the page, which says so.
                Tile {
                    objectName: "bluetoothTile"
                    visible: page.bluetoothShown
                    width: grid.cell
                    icon: Icons.BLUETOOTH
                    title: qsTr("Bluetooth")
                    subtitle: Bluetooth.tileSubtitle(page.bluetooth)
                    on: Bluetooth.isOn(page.bluetooth)
                    available: (page.bluetooth?.radio ?? "missing") !== "missing"
                    opens: true
                    onToggled: page.bluetooth.radio === "off" ? page.bluetoothPowered(true) : page.pageRequested("bluetooth")
                    onOpened: page.pageRequested("bluetooth")
                }
                Tile {
                    width: grid.cell
                    icon: Icons.CUP
                    title: qsTr("Keep awake")
                    subtitle: page.keepAwake ? qsTr("Screen stays on") : qsTr("Off")
                    on: page.keepAwake
                    onToggled: page.keepAwakeToggled()
                }
                Tile {
                    width: grid.cell
                    icon: page.micMuted ? Icons.MIC_OFF : Icons.MIC
                    title: qsTr("Microphone")
                    subtitle: !page.micAvailable ? qsTr("None") : page.micMuted ? qsTr("Muted") : qsTr("On")
                    on: page.micAvailable && page.micMuted
                    available: page.micAvailable
                    onToggled: page.micMuteToggled()
                }
            }
        }

        Reveal {
            id: sliders
            width: parent.width
            shown: page.shown
            order: 2

            Column {
                width: parent.width
                spacing: Tokens.gap

                QsSlider {
                    width: parent.width
                    glyph: Volume.icon(page.outputAvailable ? page.outputKind : "none", page.muted, page.volume)
                    label: page.outputAvailable ? page.outputName : qsTr("No output")
                    value: page.volume
                    muted: page.muted
                    available: page.outputAvailable
                    opens: true
                    onMoved: value => page.volumeMoved(value)
                    onIconClicked: page.muteToggled()
                    onOpened: page.pageRequested("sound")
                }
                QsSlider {
                    width: parent.width
                    visible: page.recorders.length > 0
                    icon: page.micMuted ? Icons.MIC_OFF : Icons.MIC
                    iconColor: Tokens.secondary
                    fill: Tokens.alpha(Tokens.secondary, Tokens.micFillOpacity)
                    label: QuickSettings.recordingText(page.recorders)
                    value: page.micVolume
                    muted: page.micMuted
                    onMoved: value => page.micVolumeMoved(value)
                    onIconClicked: page.micMuteToggled()
                }
            }
        }

        // Wireless devices and their batteries, while any is connected.
        Reveal {
            id: deviceSection
            width: parent.width
            shown: page.shown
            order: 3
            visible: page.devices.length > 0

            DevicesCard {
                id: deviceCard
                width: parent.width
                devices: page.devices
            }
        }

        // The tray's apps, all of them: the bar shows at most a few, and
        // none of those hidden from it.
        Reveal {
            id: tray
            width: parent.width
            shown: page.shown
            order: 4
            visible: page.trayApps.length > 0

            Column {
                width: parent.width
                spacing: Tokens.gap

                Item {
                    width: parent.width
                    height: trayTitle.implicitHeight

                    CardTitle {
                        id: trayTitle
                        text: qsTr("Tray")
                    }
                    CardTitle {
                        anchors.right: parent.right
                        text: qsTr("%n apps", "", page.trayApps.length)
                    }
                }

                Rectangle {
                    width: parent.width
                    height: trayScroll.height + 2 * Tokens.menuPad
                    radius: Tokens.radiusCard
                    color: Tokens.surfaceRaised

                    // A few rows, then it scrolls, so a long tray never runs
                    // under the cards below.
                    Flickable {
                        id: trayScroll
                        objectName: "trayScroll"
                        x: Tokens.menuPad
                        y: Tokens.menuPad
                        width: parent.width - 2 * Tokens.menuPad
                        height: Math.min(trayGrid.implicitHeight, Tokens.trayGridMaxRows * (Tokens.trayGridCell + Tokens.trayGridGap) - Tokens.trayGridGap)
                        contentHeight: trayGrid.implicitHeight
                        clip: true
                        boundsBehavior: Flickable.StopAtBounds

                        Grid {
                            id: trayGrid
                            width: parent.width
                            columns: Tokens.trayGridColumns
                            spacing: Tokens.trayGridGap
                            readonly property real cell: (width - (columns - 1) * spacing) / columns

                            Repeater {
                                model: page.trayApps

                                TrayIcon {
                                    id: icon
                                    required property var modelData
                                    width: trayGrid.cell
                                    height: Tokens.trayGridCell
                                    radius: Tokens.radiusControl
                                    iconSize: Tokens.trayGridIconSize
                                    source: page.trayIconSource(modelData)
                                    attention: modelData.attention
                                    dimmed: modelData.hidden
                                    onActivated: page.trayActivated(modelData)
                                    onMenuRequested: page.trayMenuRequested(modelData, icon.mapToItem(page, width / 2, height / 2))
                                    onSecondaryActivated: page.traySecondaryActivated(modelData)
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Reveal {
        id: notes
        y: sections.height + Tokens.quickSettingsGap
        width: parent.width
        height: foot.y - y - Tokens.quickSettingsGap
        visible: page.notificationsShown
        shown: page.shown
        order: 5

        NotificationList {
            anchors.fill: parent
            items: page.notifications
            doNotDisturb: page.doNotDisturb
            iconSource: page.iconSource
            now: page.now.getTime() / 1000
            onOpened: notification => page.notificationOpened(notification)
            onDismissed: id => page.notificationDismissed(id)
            onCleared: app => page.notificationsCleared(app)
            onMuted: app => page.notificationAppMuted(app)
            onDoNotDisturbToggled: page.doNotDisturbToggled()
            onSettingsOpened: page.pageRequested("notifications")
        }
    }

    Reveal {
        id: foot
        anchors.bottom: parent.bottom
        width: parent.width
        shown: page.shown
        order: 6

        ResourcesCard {
            width: parent.width
            reading: page.reading
            fill: page.fill
            onOpened: page.pageRequested("resources")
        }
    }
}
