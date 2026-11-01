pragma ComponentBehavior: Bound

import QtQuick
import "../theme"

// A row of tabs, one of them picked: `options` are {key, label}, some with
// an `icon` (an Icons path, its solid parts as `fill`) before the label.
// `accent` fills the picked one with the accent, for a choice that changes
// the machine (Prefer) rather than what the page shows.
Rectangle {
    id: segments

    required property var options
    required property string current
    property bool accent: false

    signal picked(string key)

    implicitHeight: Tokens.segmentHeight + 2 * Tokens.segmentPad
    radius: Tokens.radiusRow
    color: Tokens.surfaceRaised

    Row {
        id: row
        x: Tokens.segmentPad
        y: Tokens.segmentPad
        spacing: Tokens.segmentPad
        readonly property real cell: (segments.width - 2 * Tokens.segmentPad - spacing * (segments.options.length - 1)) / segments.options.length

        Repeater {
            model: segments.options

            Rectangle {
                id: option
                required property var modelData
                readonly property bool picked: modelData.key === segments.current
                width: row.cell
                height: Tokens.segmentHeight
                radius: Tokens.radiusSegment
                color: picked ? (segments.accent ? Tokens.primary : Tokens.selectedQuiet) : press.containsMouse ? Tokens.hover : "transparent"
                readonly property color ink: picked && segments.accent ? Tokens.onAccent : Tokens.fg

                Row {
                    anchors.centerIn: parent
                    spacing: Tokens.iconGap

                    LineIcon {
                        anchors.verticalCenter: parent.verticalCenter
                        visible: (option.modelData.icon ?? "") !== ""
                        path: option.modelData.icon ?? ""
                        fill: option.modelData.fill ?? ""
                        size: Tokens.iconSizeSmall
                        color: option.ink
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSizeSmall
                        font.weight: option.picked ? Tokens.weightTitle : Tokens.weightBody
                        color: option.ink
                        text: option.modelData.label
                    }
                }

                MouseArea {
                    id: press
                    anchors.fill: parent
                    hoverEnabled: true
                    onClicked: segments.picked(option.modelData.key)
                }
            }
        }
    }
}
