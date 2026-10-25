pragma ComponentBehavior: Bound

import QtQuick
import "../theme"

// One app in the launcher's list: its icon, its name (the letters a search
// matched in the accent) and what it is, dimmer. Hovering shows a pin
// button, which stays lit while the app is pinned; the selected row shows
// ↵. The list draws the highlight behind it; while the row launches it
// turns the accent and the row's text turns to match.
Item {
    id: row

    // A launcher.js row: {id, name, runs, description, icon}.
    required property var app
    property string iconSource: ""
    property bool selected: false
    property bool pinned: false
    // Launching: on the accent.
    property bool launching: false

    // The pointer moved over the row, to `at` on the screen.
    signal hovered(point at)
    signal clicked
    signal pinClicked

    readonly property color ink: launching ? Tokens.onAccent : Tokens.fg
    readonly property color dim: launching ? Tokens.onAccent : Tokens.fgDim

    implicitHeight: Tokens.launcherRowHeight

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onPositionChanged: event => row.hovered(mapToItem(null, event.x, event.y))
        onClicked: row.clicked()
    }

    AppIcon {
        id: icon
        anchors.verticalCenter: parent.verticalCenter
        x: Tokens.rowInset
        appId: row.app.id
        name: row.app.name
        source: row.iconSource
    }

    Row {
        id: label
        anchors.verticalCenter: parent.verticalCenter
        x: icon.x + icon.width + Tokens.pad
        width: end.x - x - Tokens.pad
        spacing: Tokens.gap
        clip: true

        Row {
            id: name
            anchors.verticalCenter: parent.verticalCenter
            Repeater {
                model: row.app.runs
                delegate: Text {
                    required property var modelData
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSize
                    font.weight: modelData.matched ? Font.DemiBold : Font.Medium
                    color: modelData.matched && !row.launching ? Tokens.primary : row.ink
                    textFormat: Text.PlainText
                    text: modelData.text
                }
            }
        }

        Text {
            anchors.verticalCenter: parent.verticalCenter
            width: Math.max(0, label.width - name.width - label.spacing)
            visible: text !== ""
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeSmall
            color: row.dim
            elide: Text.ElideRight
            textFormat: Text.PlainText
            text: row.app.description
        }
    }

    Row {
        id: end
        anchors.verticalCenter: parent.verticalCenter
        anchors.right: parent.right
        anchors.rightMargin: Tokens.rowInset
        spacing: Tokens.pad

        Rectangle {
            id: pinButton
            width: Tokens.pinButtonSize
            height: Tokens.pinButtonSize
            radius: height / 2
            color: pinMouse.containsMouse ? Tokens.hover : "transparent"
            opacity: !row.launching && (row.pinned || mouse.containsMouse || pinMouse.containsMouse) ? 1 : 0
            Behavior on opacity {
                NumberAnimation {
                    duration: Tokens.durationFast
                }
            }

            LauncherIcon {
                anchors.centerIn: parent
                width: Tokens.iconSizeSmall
                height: Tokens.iconSizeSmall
                name: "pin"
                filled: row.pinned
                color: row.pinned ? Tokens.primary : pinMouse.containsMouse ? Tokens.fg : Tokens.fgDim
            }

            MouseArea {
                id: pinMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: row.pinClicked()
            }
        }

        Text {
            anchors.verticalCenter: parent.verticalCenter
            font.family: Tokens.fontMono
            font.pixelSize: Tokens.fontSizeTiny
            color: row.dim
            opacity: row.selected ? 1 : 0
            text: "↵"
        }
    }
}
