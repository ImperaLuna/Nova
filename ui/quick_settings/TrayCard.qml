pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../tray"

// The tray's apps in quick settings, all of them: the bar shows at most a
// few, and none of those hidden from it. A few rows, then it scrolls, so a
// long tray never runs under the cards below. Only properties in and
// signals out, so ui/tests can drive it.
Column {
    id: card

    // Every tray app, hidden ones too (TrayService.all), and what an Image
    // shows for one.
    property var apps: []
    property var iconSource: entry => ""

    // An app's icon was clicked; `at` is its middle, in this card.
    signal activated(var entry)
    signal menuRequested(var entry, point at)
    signal secondaryActivated(var entry)

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
            text: qsTr("%n apps", "", card.apps.length)
        }
    }

    Rectangle {
        width: parent.width
        height: trayScroll.height + 2 * Tokens.menuPad
        radius: Tokens.radiusCard
        color: Tokens.surfaceRaised

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
                    model: card.apps

                    TrayIcon {
                        id: icon
                        required property var modelData
                        width: trayGrid.cell
                        height: Tokens.trayGridCell
                        radius: Tokens.radiusControl
                        iconSize: Tokens.trayGridIconSize
                        source: card.iconSource(modelData)
                        attention: modelData.attention
                        dimmed: modelData.hidden
                        onActivated: card.activated(modelData)
                        onMenuRequested: card.menuRequested(modelData, icon.mapToItem(card, width / 2, height / 2))
                        onSecondaryActivated: card.secondaryActivated(modelData)
                    }
                }
            }
        }
    }
}
