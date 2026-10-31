pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../volume/volume.js" as Volume
import "quick_settings.js" as QuickSettings

// The sound page: the output in use with its volume, then tabs for every
// output and input to switch to, and each app's own volume. Lists are
// SoundService's plain objects ({id, name, kind, link, volume, muted,
// current}; apps also have `ids` and `icon`).
Item {
    id: page

    property bool outputAvailable: false
    property string outputName: ""
    property string outputKind: "none"
    property real volume: 0
    property bool muted: false
    property bool inputAvailable: false
    property string inputName: ""
    property real inputVolume: 0
    property bool inputMuted: false
    property var outputs: []
    property var inputs: []
    property var apps: []
    // "output", "input" or "apps".
    property string tab: "apps"

    signal back
    // Room the top row leaves on its right (the pill's buttons).
    property real headerInset: 0
    signal volumeMoved(real value)
    signal muteToggled
    signal inputVolumeMoved(real value)
    signal inputMuteToggled
    signal outputPicked(int id)
    signal inputPicked(int id)
    // An `apps` entry's stream ids, and their new volume.
    signal appVolumeMoved(var ids, real value)
    // An `apps` entry's stream ids, to mute all or unmute all.
    signal appMuteToggled(var ids, bool muted)
    // An icon file (novad's) to an image source, or "" (AppIcons.source,
    // given by the drawer so tests can do without).
    property var iconSource: name => ""

    // The card on top shows the input in use on the Input tab, else the
    // output.
    readonly property bool input: tab === "input"

    function kindIcon(kind: string): string {
        return {
            headphones: Icons.HEADPHONES,
            bluetooth: Icons.BLUETOOTH,
            display: Icons.MONITOR
        }[kind] ?? Icons.SPEAKER;
    }

    Column {
        width: page.width
        spacing: Tokens.quickSettingsGap

        PageHeader {
            width: parent.width - page.headerInset
            title: qsTr("Sound")
            onBack: page.back()
        }

        Rectangle {
            width: parent.width
            height: current.implicitHeight + 2 * Tokens.pad
            radius: Tokens.radiusCard
            color: Tokens.surfaceRaised

            Column {
                id: current
                x: Tokens.pad
                y: Tokens.pad
                width: parent.width - 2 * Tokens.pad
                spacing: Tokens.rowInset

                // The device in use: the slider's icon below is its one icon.
                Column {
                    Text {
                        width: current.width
                        elide: Text.ElideRight
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        font.weight: Font.DemiBold
                        color: Tokens.fg
                        textFormat: Text.PlainText
                        text: page.input ? (page.inputAvailable ? page.inputName : qsTr("No input")) : page.outputAvailable ? page.outputName : qsTr("No output")
                    }
                    // Kept when empty, so the card is the same height on
                    // every tab.
                    Text {
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSizeTiny
                        color: Tokens.fgDim
                        text: QuickSettings.currentLink(page.input ? page.inputs : page.outputs)
                    }
                }

                QsSlider {
                    width: parent.width
                    visible: !page.input
                    glyph: Volume.icon(page.outputAvailable ? page.outputKind : "none", page.muted, page.volume)
                    label: qsTr("Output")
                    value: page.volume
                    muted: page.muted
                    available: page.outputAvailable
                    onMoved: value => page.volumeMoved(value)
                    onIconClicked: page.muteToggled()
                }

                QsSlider {
                    width: parent.width
                    visible: page.input
                    icon: page.inputMuted ? Icons.MIC_OFF : Icons.MIC
                    label: qsTr("Input")
                    value: page.inputVolume
                    muted: page.inputMuted
                    available: page.inputAvailable
                    onMoved: value => page.inputVolumeMoved(value)
                    onIconClicked: page.inputMuteToggled()
                }
            }
        }

        Segments {
            width: parent.width
            current: page.tab
            options: [
                {
                    key: "apps",
                    label: qsTr("Apps")
                },
                {
                    key: "output",
                    label: qsTr("Output")
                },
                {
                    key: "input",
                    label: qsTr("Input")
                }
            ]
            onPicked: key => page.tab = key
        }

        // The tab's list, scrolling once it's longer than the page.
        Flickable {
            id: lists
            objectName: "lists"
            width: parent.width
            height: Math.max(0, page.height - y)
            contentHeight: tabLists.height
            clip: true
            boundsBehavior: Flickable.StopAtBounds

            Column {
                id: tabLists
                width: lists.width

                Column {
                    width: parent.width
                    spacing: Tokens.labelNudge * 2
                    visible: page.tab !== "apps"

                    // By count, so a volume change elsewhere in the lists doesn't
                    // rebuild the rows.
                    Repeater {
                        id: rows
                        readonly property var shown: page.tab === "output" ? page.outputs : page.tab === "input" ? page.inputs : []
                        model: shown.length

                        ListRow {
                            required property int index
                            // Past the end for a moment while the tab switches.
                            readonly property var modelData: rows.shown[index] ?? {
                                id: -1,
                                name: "",
                                kind: "",
                                link: "",
                                current: false
                            }
                            width: parent.width
                            icon: page.tab === "input" ? Icons.MIC : page.kindIcon(modelData.kind)
                            title: modelData.name
                            subtitle: QuickSettings.soundSubtitle(page.tab === "input" ? "" : modelData.kind, modelData.link ?? "")
                            status: modelData.current ? qsTr("In use") : ""
                            selected: modelData.current
                            onClicked: page.tab === "input" ? page.inputPicked(modelData.id) : page.outputPicked(modelData.id)
                        }
                    }
                }

                Column {
                    width: parent.width
                    spacing: Tokens.gap
                    visible: page.tab === "apps"

                    // By count: every volume change gives a new `apps`, and a
                    // rebuilt slider would drop the drag under the pointer.
                    Repeater {
                        model: page.tab === "apps" ? page.apps.length : 0

                        QsSlider {
                            required property int index
                            readonly property var stream: page.apps[index] ?? {
                                ids: [],
                                icon: "",
                                name: "",
                                volume: 0,
                                muted: false
                            }
                            width: parent.width
                            app: true
                            appIcon: page.iconSource(stream.icon)
                            label: stream.name
                            value: stream.volume
                            muted: stream.muted
                            onMoved: value => page.appVolumeMoved(stream.ids, value)
                            onIconClicked: page.appMuteToggled(stream.ids, !stream.muted)
                        }
                    }

                    Text {
                        visible: page.apps.length === 0
                        leftPadding: Tokens.pad
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSizeSmall
                        color: Tokens.fgDim
                        text: qsTr("Nothing playing")
                    }
                }
            }
        }
    }
}
