import QtQuick
import "../theme"

// Marks the screen the wallpaper carousel picks for, while it picks for
// one: a ring round the screen's edge and a tag under the bar, fading in
// and out with `shown`. Drawn by the carousel on its own screen and by
// WallpaperPreview on the others.
Item {
    id: picking

    property bool shown: false

    opacity: shown ? 1 : 0
    visible: opacity > 0
    Behavior on opacity {
        NumberAnimation {
            duration: Tokens.durationFast
        }
    }

    Rectangle {
        anchors.fill: parent
        color: "transparent"
        border.width: Tokens.carouselScreenRing
        border.color: Tokens.fg
    }

    Rectangle {
        anchors.horizontalCenter: parent.horizontalCenter
        y: Tokens.barGap + Tokens.barHeight + Tokens.carouselLabelGap
        width: tag.implicitWidth + 2 * Tokens.carouselLabelPad
        height: Tokens.carouselChipHeight + Tokens.barGap * 2
        radius: height / 2
        color: Tokens.surface

        Row {
            id: tag
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
                font.weight: Tokens.weightLabel
                color: Tokens.fg
                text: qsTr("Picking for this screen")
            }
        }
    }
}
