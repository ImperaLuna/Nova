pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "quick_settings.js" as QuickSettings

// The resources card at the sidebar's foot: CPU, memory, GPU and the CPU's
// temperature as rings, from a resources event. Readings novad doesn't
// have show a dash. Clicking it opens the resources page.
Item {
    id: card

    // A resources event's data, or null before the first.
    property var reading: null
    // The rings fill from 0 to their readings as this goes from 0 to 1.
    property real fill: 1

    signal opened

    readonly property var rings: {
        const now = reading;
        const dash = "—";
        return [
            {
                label: qsTr("CPU"),
                value: now ? now.cpu / 100 : 0,
                reading: now ? QuickSettings.percent(now.cpu) : dash
            },
            {
                label: qsTr("RAM"),
                value: now ? QuickSettings.share(now.memory_used, now.memory_total) : 0,
                reading: now ? QuickSettings.shortBytes(now.memory_used) : dash
            },
            {
                label: qsTr("GPU"),
                value: now?.gpu != null ? now.gpu / 100 : 0,
                reading: now?.gpu != null ? QuickSettings.percent(now.gpu) : dash
            },
            {
                label: qsTr("Temp"),
                value: now?.temperature != null ? now.temperature / 100 : 0,
                reading: now?.temperature != null ? `${Math.round(now.temperature)}°` : dash
            }
        ];
    }

    implicitHeight: column.implicitHeight + 2 * Tokens.pad

    Rectangle {
        anchors.fill: parent
        radius: Tokens.radiusCard
        color: press.containsMouse ? Tokens.hover : Tokens.surfaceRaised
    }

    MouseArea {
        id: press
        anchors.fill: parent
        hoverEnabled: true
        onClicked: card.opened()
    }

    Column {
        id: column
        x: Tokens.pad
        y: Tokens.pad
        width: card.width - 2 * Tokens.pad
        spacing: Tokens.gap

        Item {
            width: parent.width
            height: title.implicitHeight

            CardTitle {
                id: title
                text: qsTr("Resources")
            }
            LineIcon {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                path: Icons.RIGHT
                size: Tokens.iconSizeSmall
                color: Tokens.fgSoft
            }
        }

        Row {
            id: row
            readonly property real cell: width / card.rings.length
            width: parent.width

            Repeater {
                model: card.rings

                Item {
                    id: slot
                    required property var modelData
                    width: row.cell
                    height: ring.implicitHeight

                    Ring {
                        id: ring
                        anchors.horizontalCenter: parent.horizontalCenter
                        value: slot.modelData.value * card.fill
                        reading: slot.modelData.reading
                        label: slot.modelData.label
                    }
                }
            }
        }
    }
}
