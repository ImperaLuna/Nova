pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../volume/volume.js" as Volume
import "quick_settings.js" as QuickSettings

// The sidebar's first page: the header, the toggle tiles, the sliders, and
// the resources card pinned at the foot. The space between is kept for
// notifications (IMP-61). Everything comes in as properties and goes out
// as signals, so ui/tests can drive it without PipeWire or novad.
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

    // Keep awake and the microphone.
    property bool keepAwake: false
    property bool micAvailable: false
    property bool micMuted: false

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
    // Room the top row leaves on its right (the pill's buttons).
    property real headerInset: 0

    signal keepAwakeToggled
    signal micMuteToggled
    signal volumeMoved(real value)
    signal muteToggled
    signal micVolumeMoved(real value)
    signal pageRequested(string page)

    readonly property bool revealing: [header, tiles, sliders, foot].some(section => section.running)

    Column {
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
                detail: page.host ? `${page.host.host} · ${QuickSettings.uptime(page.now.getTime() / 1000 - page.host.booted_at)}` : ""
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
                    fill: Qt.rgba(Tokens.secondary.r, Tokens.secondary.g, Tokens.secondary.b, Tokens.micFillOpacity)
                    label: QuickSettings.recordingText(page.recorders)
                    value: page.micVolume
                    muted: page.micMuted
                    onMoved: value => page.micVolumeMoved(value)
                    onIconClicked: page.micMuteToggled()
                }
            }
        }
    }

    Reveal {
        id: foot
        anchors.bottom: parent.bottom
        width: parent.width
        shown: page.shown
        order: 3

        ResourcesCard {
            width: parent.width
            reading: page.reading
            fill: page.fill
            onOpened: page.pageRequested("resources")
        }
    }
}
