pragma ComponentBehavior: Bound

import QtQuick
import "../theme"

// A row of tabs, one of them picked: `options` are {key, label}.
Rectangle {
    id: segments

    required property var options
    required property string current

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
                color: picked ? Tokens.selectedQuiet : press.containsMouse ? Tokens.hover : "transparent"

                Text {
                    anchors.centerIn: parent
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeSmall
                    font.weight: option.picked ? Font.DemiBold : Font.Normal
                    color: Tokens.fg
                    text: option.modelData.label
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
