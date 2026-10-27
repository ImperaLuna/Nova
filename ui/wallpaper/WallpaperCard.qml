pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Effects
import "../theme"
import "../text/files.js" as Files

// One image in the wallpaper carousel: the picture cut to the card's
// rounding (a quiet card until it loads), dimmed by `dim`, ringed while it's
// the picked one, marked "Now" while it's the screen's wallpaper, and with
// the names of the other screens picking it.
Item {
    id: card

    // An absolute file path, or "" while it's out of sight.
    property string path: ""
    property bool picked: false
    property bool now: false
    property var marks: []
    // How much darker it is than the middle card (0 to 1).
    property real dim: 0

    readonly property bool loaded: image.status === Image.Ready

    Rectangle {
        anchors.fill: parent
        radius: Tokens.radiusCard
        color: Tokens.carouselLoading
        visible: !card.loaded
    }

    Image {
        id: image
        anchors.fill: parent
        source: card.path !== "" ? Files.fileUrl(card.path) : ""
        sourceSize: Qt.size(card.width * Screen.devicePixelRatio, card.height * Screen.devicePixelRatio)
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        visible: card.loaded
        layer.enabled: card.loaded
        layer.effect: MultiEffect {
            maskEnabled: true
            maskSource: mask
        }
    }

    // The card's shape, only ever drawn as its mask.
    Rectangle {
        id: mask
        width: card.width
        height: card.height
        radius: Tokens.radiusCard
        visible: false
        layer.enabled: true
    }

    Rectangle {
        anchors.fill: parent
        radius: Tokens.radiusCard
        color: Tokens.backdrop
        opacity: card.dim
        visible: card.dim > 0
    }

    Rectangle {
        anchors.fill: parent
        anchors.margins: -Tokens.carouselRing
        radius: Tokens.radiusCard + Tokens.carouselRing
        color: "transparent"
        border.width: Tokens.carouselRing
        border.color: Tokens.fg
        visible: card.picked
    }

    Rectangle {
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.margins: Tokens.pad
        visible: card.now
        width: nowRow.implicitWidth + 2 * Tokens.chipPad
        height: Tokens.chipHeight
        radius: height / 2
        color: Tokens.surface

        Row {
            id: nowRow
            anchors.centerIn: parent
            spacing: Tokens.iconGap

            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                width: Tokens.legendDot
                height: Tokens.legendDot
                radius: width / 2
                color: Tokens.primary
            }

            Text {
                anchors.verticalCenter: parent.verticalCenter
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeSmall
                font.weight: Font.Medium
                color: Tokens.fg
                text: qsTr("Now")
            }
        }
    }

    Row {
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.margins: Tokens.pad
        spacing: Tokens.iconGap

        Repeater {
            model: card.marks

            Rectangle {
                required property string modelData
                width: mark.implicitWidth + 2 * Tokens.chipPad
                height: Tokens.chipHeight
                radius: height / 2
                color: Tokens.surface

                Text {
                    id: mark
                    anchors.centerIn: parent
                    font.family: Tokens.fontMono
                    font.pixelSize: Tokens.fontSizeTiny
                    font.weight: Font.Medium
                    color: Tokens.fg
                    textFormat: Text.PlainText
                    text: parent.modelData
                }
            }
        }
    }
}
