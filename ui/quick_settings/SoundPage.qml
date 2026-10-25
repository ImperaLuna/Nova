pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../volume/volume.js" as Volume
import "quick_settings.js" as QuickSettings

// The sound page: the output in use with its volume, then tabs for every
// output and input to switch to, and each app's own volume. Lists are
// SoundService's plain objects ({id, name, kind, volume, muted, current}).
Item {
    id: page

    property bool outputAvailable: false
    property string outputName: ""
    property string outputKind: "none"
    property real volume: 0
    property bool muted: false
    property var outputs: []
    property var inputs: []
    property var apps: []
    // "output", "input" or "apps".
    property string tab: "output"

    signal back
    signal volumeMoved(real value)
    signal muteToggled
    signal outputPicked(int id)
    signal inputPicked(int id)
    signal appVolumeMoved(int id, real value)

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
            width: parent.width
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

                Row {
                    spacing: Tokens.rowInset

                    Rectangle {
                        width: Tokens.logoSize
                        height: Tokens.logoSize
                        radius: width / 2
                        color: Tokens.primary

                        LineIcon {
                            anchors.centerIn: parent
                            path: page.kindIcon(page.outputKind)
                            size: Tokens.iconSizeMenu
                            color: Tokens.onAccent
                        }
                    }
                    Column {
                        anchors.verticalCenter: parent.verticalCenter
                        Text {
                            width: current.width - Tokens.logoSize - Tokens.rowInset
                            elide: Text.ElideRight
                            font.family: Tokens.fontSans
                            font.pixelSize: Tokens.fontSize
                            font.weight: Font.DemiBold
                            color: Tokens.fg
                            textFormat: Text.PlainText
                            text: page.outputAvailable ? page.outputName : qsTr("No output")
                        }
                        Text {
                            font.family: Tokens.fontSans
                            font.pixelSize: Tokens.fontSizeTiny
                            color: Tokens.fgDim
                            text: QuickSettings.kindLabel(page.outputKind)
                        }
                    }
                }

                QsSlider {
                    width: parent.width
                    glyph: Volume.icon(page.outputAvailable ? page.outputKind : "none", page.muted, page.volume)
                    label: qsTr("Output")
                    value: page.muted ? 0 : page.volume
                    available: page.outputAvailable
                    onMoved: value => page.volumeMoved(value)
                    onIconClicked: page.muteToggled()
                }
            }
        }

        Segments {
            width: parent.width
            current: page.tab
            options: [
                {
                    key: "output",
                    label: qsTr("Output")
                },
                {
                    key: "input",
                    label: qsTr("Input")
                },
                {
                    key: "apps",
                    label: qsTr("Apps")
                }
            ]
            onPicked: key => page.tab = key
        }

        Column {
            width: parent.width
            spacing: Tokens.labelNudge * 2
            visible: page.tab !== "apps"

            Repeater {
                model: page.tab === "output" ? page.outputs : page.tab === "input" ? page.inputs : []

                ListRow {
                    required property var modelData
                    width: parent.width
                    icon: page.tab === "input" ? Icons.MIC : page.kindIcon(modelData.kind)
                    title: modelData.name
                    subtitle: page.tab === "input" ? "" : QuickSettings.kindLabel(modelData.kind)
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

            Repeater {
                model: page.tab === "apps" ? page.apps : []

                QsSlider {
                    required property var modelData
                    width: parent.width
                    icon: Icons.APP
                    label: modelData.name
                    value: modelData.muted ? 0 : modelData.volume
                    onMoved: value => page.appVolumeMoved(modelData.id, value)
                }
            }

            Text {
                visible: page.apps.length === 0
                leftPadding: Tokens.pad
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeSmall
                color: Tokens.fgDim
                text: qsTr("No app is playing sound")
            }
        }
    }
}
