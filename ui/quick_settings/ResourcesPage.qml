pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "quick_settings.js" as QuickSettings

// The resources page: CPU and memory over the last minutes, and the
// busiest processes now, from novad's resources events.
Item {
    id: page

    // A resources event's data, or null before the first.
    property var reading: null
    signal back

    readonly property real memoryShare: reading ? QuickSettings.share(reading.memory_used, reading.memory_total) * 100 : 0

    component Legend: Row {
        id: legend
        required property color color
        required property string text
        spacing: Tokens.iconGap

        Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            width: Tokens.legendDot
            height: width
            radius: width / 2
            color: legend.color
        }
        CardTitle {
            text: legend.text
        }
    }

    component Cell: Text {
        font.family: Tokens.fontMono
        font.pixelSize: Tokens.fontSizeSmall
        horizontalAlignment: Text.AlignRight
        width: Tokens.processNumberWidth
    }

    Column {
        width: page.width
        spacing: Tokens.quickSettingsGap

        PageHeader {
            width: parent.width
            title: qsTr("Resources")
            onBack: page.back()
        }

        Rectangle {
            width: parent.width
            height: graphColumn.implicitHeight + 2 * Tokens.pad
            radius: Tokens.radiusCard
            color: Tokens.surfaceRaised

            Column {
                id: graphColumn
                x: Tokens.pad
                y: Tokens.pad
                width: parent.width - 2 * Tokens.pad
                spacing: Tokens.gap

                Row {
                    spacing: Tokens.pad
                    Legend {
                        color: Tokens.primary
                        text: qsTr("CPU %1").arg(page.reading ? QuickSettings.percent(page.reading.cpu) : "—")
                    }
                    Legend {
                        color: Tokens.secondary
                        text: qsTr("RAM %1").arg(page.reading ? QuickSettings.percent(page.memoryShare) : "—")
                    }
                }

                Graph {
                    width: parent.width
                    history: page.reading?.history ?? []
                    span: page.reading?.history_size ?? 1
                }

                Item {
                    width: parent.width
                    height: past.implicitHeight
                    Text {
                        id: past
                        font.family: Tokens.fontMono
                        font.pixelSize: Tokens.fontSizeMicro
                        color: Tokens.fgDim
                        text: qsTr("−3 min")
                    }
                    Text {
                        anchors.right: parent.right
                        font.family: Tokens.fontMono
                        font.pixelSize: Tokens.fontSizeMicro
                        color: Tokens.fgDim
                        text: qsTr("now")
                    }
                }
            }
        }

        Column {
            width: parent.width

            Item {
                width: parent.width
                height: Tokens.chipHeight
                Text {
                    x: Tokens.pad
                    anchors.verticalCenter: parent.verticalCenter
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeTiny
                    color: Tokens.fgDim
                    text: qsTr("Top processes")
                }
                Row {
                    anchors.right: parent.right
                    anchors.rightMargin: Tokens.pad
                    anchors.verticalCenter: parent.verticalCenter
                    Cell {
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSizeTiny
                        color: Tokens.fgDim
                        text: qsTr("CPU")
                    }
                    Cell {
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSizeTiny
                        color: Tokens.fgDim
                        text: qsTr("Memory")
                    }
                }
            }

            Repeater {
                model: page.reading?.processes ?? []

                Item {
                    id: process
                    required property var modelData
                    width: parent.width
                    height: Tokens.processRowHeight

                    Text {
                        x: Tokens.pad
                        anchors.verticalCenter: parent.verticalCenter
                        width: numbers.x - x - Tokens.gap
                        elide: Text.ElideRight
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSizeSmall
                        color: Tokens.fg
                        textFormat: Text.PlainText
                        text: process.modelData.name
                    }
                    Row {
                        id: numbers
                        anchors.right: parent.right
                        anchors.rightMargin: Tokens.pad
                        anchors.verticalCenter: parent.verticalCenter
                        Cell {
                            color: Tokens.fg
                            text: `${process.modelData.cpu.toFixed(1)}%`
                        }
                        Cell {
                            color: Tokens.fgDim
                            text: QuickSettings.bytes(process.modelData.memory)
                        }
                    }
                }
            }
        }
    }
}
