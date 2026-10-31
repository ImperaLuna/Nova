pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "devices.js" as Devices

// The wireless mice, keyboards, headsets and controllers that are connected
// now, each with how it connects and its battery. `devices` is a
// peripherals event's list; novad leaves out whatever isn't connected.
Column {
    id: card

    property var devices: []
    // Rows fading in or out, the card resizing or a battery easing.
    readonly property bool running: settling.running || resize.running

    spacing: Tokens.gap
    onDevicesChanged: {
        rowModel.sync(devices);
        settling.restart();
    }
    Component.onCompleted: rowModel.sync(devices)

    // After a new reading, rows fade and batteries ease for at most this
    // long (Transition.running can stay set once a remove has finished).
    Timer {
        id: settling
        interval: Math.max(Tokens.durationFill, Tokens.durationPanel)
    }

    DeviceList {
        id: rowModel
    }

    Item {
        width: parent.width
        height: title.implicitHeight

        CardTitle {
            id: title
            text: qsTr("Devices")
        }
    }

    Rectangle {
        width: parent.width
        height: list.height + 2 * Tokens.menuPad
        radius: Tokens.radiusCard
        color: Tokens.surfaceRaised
        Behavior on height {
            NumberAnimation {
                id: resize
                duration: Tokens.durationPanel
                easing.type: Easing.OutCubic
            }
        }

        ListView {
            id: list
            objectName: "deviceRows"
            x: Tokens.menuPad
            y: Tokens.menuPad
            width: parent.width - 2 * Tokens.menuPad
            // A few rows, then it scrolls, so many devices never push the
            // cards below off the sidebar.
            height: Math.min(contentHeight, Tokens.deviceListMaxRows * Tokens.deviceRowHeight)
            interactive: contentHeight > height
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            model: rowModel

            add: Transition {
                NumberAnimation {
                    property: "opacity"
                    from: 0
                    to: 1
                    duration: Tokens.durationPanel
                }
            }
            remove: Transition {
                NumberAnimation {
                    property: "opacity"
                    to: 0
                    duration: Tokens.durationPanel
                }
            }
            displaced: Transition {
                NumberAnimation {
                    property: "y"
                    duration: Tokens.durationPanel
                    easing.type: Easing.OutCubic
                }
            }

            delegate: Item {
                id: row
                required property string deviceId
                required property string name
                required property string kind
                required property string link
                required property int percent
                required property string level
                required property bool charging
                readonly property bool low: Devices.low(percent, level, charging)
                readonly property bool reported: percent >= 0 || level !== ""

                width: list.width
                height: Tokens.deviceRowHeight

                LineIcon {
                    id: glyph
                    anchors.verticalCenter: parent.verticalCenter
                    x: Tokens.gap
                    path: Devices.icon(row.kind)
                    size: Tokens.deviceIconSize
                }

                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    x: glyph.x + glyph.width + Tokens.gap
                    width: chip.x - x - Tokens.gap
                    elide: Text.ElideRight
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSize
                    color: Tokens.fg
                    textFormat: Text.PlainText
                    text: row.name
                }

                Rectangle {
                    id: chip
                    visible: row.link !== ""
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.right: meter.left
                    anchors.rightMargin: Tokens.gap
                    width: visible ? chipRow.implicitWidth + 2 * Tokens.deviceChipPad : 0
                    height: Tokens.deviceChipHeight
                    radius: height / 2
                    color: Tokens.track

                    Row {
                        id: chipRow
                        anchors.centerIn: parent
                        spacing: Tokens.deviceChipGap

                        LineIcon {
                            anchors.verticalCenter: parent.verticalCenter
                            path: Devices.linkIcon(row.link)
                            size: Tokens.deviceLinkIconSize
                            color: Tokens.fgSoft
                        }
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            font.family: Tokens.fontSans
                            font.pixelSize: Tokens.fontSizeMicro
                            color: Tokens.fgSoft
                            text: ({
                                    dongle: qsTr("2.4 GHz"),
                                    bluetooth: qsTr("Bluetooth"),
                                    wired: qsTr("Wired")
                                })[row.link] ?? ""
                        }
                    }
                }

                DeviceBattery {
                    id: meter
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.right: amount.left
                    anchors.rightMargin: Tokens.gap
                    visible: row.reported
                    fill: Devices.fill(row.percent, row.level)
                    low: row.low
                    charging: row.charging
                }

                Text {
                    id: amount
                    objectName: "amount"
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.right: parent.right
                    anchors.rightMargin: Tokens.gap
                    width: Tokens.deviceLevelWidth
                    horizontalAlignment: Text.AlignRight
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeSmall
                    color: !row.reported ? Tokens.fgSoft : row.low ? Tokens.error : row.level !== "" ? Tokens.fgSoft : Tokens.fg
                    text: row.percent >= 0 ? `${row.percent}%` : ({
                            critical: qsTr("Critical"),
                            low: qsTr("Low"),
                            normal: qsTr("Normal"),
                            high: qsTr("High"),
                            full: qsTr("Full")
                        })[row.level] ?? "—"
                }
            }
        }
    }
}
