pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "quick_settings.js" as QuickSettings

// The resources page: CPU and memory over the last minutes, and the
// apps using the most memory now (each with all its processes), from
// novad's resources events. An app's row can end it.
Item {
    id: page

    // A resources event's data, or null before the first.
    property var reading: null
    signal back
    // Room the top row leaves on its right (the pill's buttons).
    property real headerInset: 0
    // End on an app's row, clicked twice: novad's end_app for its key.
    signal endRequested(string key)

    readonly property var apps: reading?.apps ?? []
    // The app whose End waits for its second click, by key.
    property string armed: ""
    onArmedChanged: {
        if (armed !== "")
            disarm.restart();
    }
    Timer {
        id: disarm
        interval: Tokens.durationEndConfirm
        onTriggered: page.armed = ""
    }

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
            width: parent.width - page.headerInset
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
                    text: qsTr("Apps by memory")
                }
                Row {
                    anchors.right: parent.right
                    anchors.rightMargin: Tokens.segmentPad + Tokens.endButtonWidth
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

            // By count, so a new reading every few seconds doesn't rebuild
            // the rows (and drop a hover or an armed End).
            Repeater {
                model: page.apps.length

                Rectangle {
                    id: row
                    required property int index
                    readonly property var app: page.apps[index] ?? {
                        name: "",
                        key: "",
                        processes: 0,
                        cpu: 0,
                        memory: 0,
                        protected: true
                    }
                    readonly property bool armed: page.armed !== "" && page.armed === app.key
                    objectName: `app-${app.key}`
                    width: parent.width
                    height: Tokens.processRowHeight
                    radius: Tokens.radiusSegment
                    color: hover.containsMouse ? Tokens.surfaceRaised : "transparent"

                    MouseArea {
                        id: hover
                        anchors.fill: parent
                        hoverEnabled: true
                        acceptedButtons: Qt.NoButton
                    }

                    Row {
                        x: Tokens.pad
                        anchors.verticalCenter: parent.verticalCenter
                        width: numbers.x - x - Tokens.gap
                        spacing: Tokens.iconGap
                        Text {
                            id: appName
                            width: Math.min(implicitWidth, parent.width - count.width - parent.spacing)
                            elide: Text.ElideRight
                            font.family: Tokens.fontSans
                            font.pixelSize: Tokens.fontSizeSmall
                            color: Tokens.fg
                            textFormat: Text.PlainText
                            text: row.app.name
                        }
                        Text {
                            id: count
                            visible: row.app.processes > 1
                            anchors.baseline: appName.baseline
                            font.family: Tokens.fontMono
                            font.pixelSize: Tokens.fontSizeMicro
                            color: Tokens.fgDim
                            text: `×${row.app.processes}`
                        }
                    }
                    Row {
                        id: numbers
                        anchors.right: parent.right
                        anchors.rightMargin: Tokens.segmentPad
                        anchors.verticalCenter: parent.verticalCenter
                        Cell {
                            anchors.verticalCenter: parent.verticalCenter
                            color: Tokens.fg
                            text: `${row.app.cpu.toFixed(1)}%`
                        }
                        Cell {
                            anchors.verticalCenter: parent.verticalCenter
                            color: Tokens.fgDim
                            text: QuickSettings.bytes(row.app.memory)
                        }
                        // ✕ while hovered; a click arms it as "End", and a
                        // second ends the app. Nova and the session have none.
                        Item {
                            width: Tokens.endButtonWidth
                            height: Tokens.endButtonHeight
                            Rectangle {
                                anchors.right: parent.right
                                width: row.armed ? parent.width : height
                                height: parent.height
                                radius: Tokens.radiusSegment
                                visible: !row.app.protected && (hover.containsMouse || end.containsMouse || row.armed)
                                color: row.armed ? Tokens.error : end.containsMouse ? Tokens.hover : "transparent"

                                LineIcon {
                                    anchors.centerIn: parent
                                    visible: !row.armed
                                    path: Icons.CLOSE
                                    size: Tokens.endIconSize
                                    color: end.containsMouse ? Tokens.fg : Tokens.fgDim
                                }
                                Text {
                                    anchors.centerIn: parent
                                    visible: row.armed
                                    font.family: Tokens.fontSans
                                    font.pixelSize: Tokens.fontSizeMicro
                                    font.weight: Font.DemiBold
                                    color: Tokens.onAccent
                                    text: qsTr("End")
                                }
                                MouseArea {
                                    id: end
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    onClicked: {
                                        if (row.armed) {
                                            page.armed = "";
                                            page.endRequested(row.app.key);
                                        } else {
                                            page.armed = row.app.key;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
