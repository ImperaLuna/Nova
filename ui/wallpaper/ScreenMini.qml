pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../text/files.js" as Files

// One screen in the wallpaper carousel's screen picker: its pick, small,
// shaped like the screen (its size is set by the picker), with the
// screen's name under it; ringed while the deck picks for it.
Item {
    id: mini

    required property string name
    // An absolute file path, or "" while the screen shows nothing.
    property string path: ""
    property bool chosen: false
    signal clicked

    opacity: chosen ? 1 : Tokens.carouselMiniIdle

    Rectangle {
        anchors.fill: parent
        radius: Tokens.carouselMiniRadius
        color: Tokens.carouselLoading
    }

    Image {
        anchors.fill: parent
        source: mini.path !== "" ? Files.fileUrl(mini.path) : ""
        sourceSize: Qt.size(mini.width * Screen.devicePixelRatio, mini.height * Screen.devicePixelRatio)
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
    }

    Rectangle {
        anchors.fill: parent
        anchors.margins: mini.chosen ? -Tokens.carouselMiniRing : 0
        radius: Tokens.carouselMiniRadius - anchors.margins
        color: "transparent"
        border.width: mini.chosen ? Tokens.carouselMiniRing : 1
        border.color: mini.chosen ? Tokens.fg : Tokens.glassRim
    }

    Text {
        anchors.top: parent.bottom
        anchors.topMargin: Tokens.carouselMiniRing + 1
        anchors.horizontalCenter: parent.horizontalCenter
        font.family: Tokens.fontMono
        font.pixelSize: Tokens.fontSizeTiny
        color: mini.chosen ? Tokens.fg : Tokens.fgDim
        textFormat: Text.PlainText
        text: mini.name
    }

    MouseArea {
        anchors.fill: parent
        anchors.margins: -Tokens.carouselMiniRing
        onClicked: mini.clicked()
    }
}
