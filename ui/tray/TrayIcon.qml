import QtQuick
import "../theme"

// One tray app's icon, as a round button: in the right pill's strip and in
// quick settings' Tray row. A left click activates it, a right click asks
// for its menu and a middle click is its secondary action; the wheel is
// left to the strip, which scrolls. The parent decides what each means.
Rectangle {
    id: button

    // What the icon shows (TrayService.iconSource).
    property string source: ""
    property bool attention: false
    property int iconSize: Tokens.trayIconSize
    // Shown faded: hidden from the bar (quick settings lists those too).
    property bool dimmed: false
    // How much of it shows, 0 to 1: the strip fades icons at its ends.
    property real fade: 1

    signal activated
    signal menuRequested
    signal secondaryActivated

    width: Tokens.pillButtonSize
    height: Tokens.pillButtonSize
    radius: height / 2
    color: press.containsMouse ? Tokens.hover : "transparent"

    Behavior on color {
        ColorAnimation {
            duration: Tokens.durationFast
        }
    }

    Image {
        id: picture
        anchors.centerIn: parent
        width: button.iconSize
        height: button.iconSize
        sourceSize.width: width * 2
        sourceSize.height: height * 2
        source: button.source
        smooth: true
        mipmap: true
        asynchronous: true
        opacity: (button.dimmed ? Tokens.dimOpacity : 1) * button.fade
    }

    // An icon that didn't load still takes clicks: a plain tile marks it.
    Rectangle {
        anchors.fill: picture
        visible: picture.status !== Image.Ready
        opacity: button.fade
        radius: Tokens.trayIconRadius
        color: Tokens.surfaceRaised
    }

    Rectangle {
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.margins: Tokens.trayDotRing
        width: Tokens.trayDotSize
        height: Tokens.trayDotSize
        radius: width / 2
        color: Tokens.primary
        border.width: Tokens.trayDotRing
        border.color: Tokens.bg
        scale: button.attention ? 1 : 0
        opacity: button.fade

        Behavior on scale {
            NumberAnimation {
                duration: Tokens.durationPanel
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }
    }

    MouseArea {
        id: press
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton
        onClicked: mouse => {
            if (mouse.button === Qt.RightButton)
                button.menuRequested();
            else if (mouse.button === Qt.MiddleButton)
                button.secondaryActivated();
            else
                button.activated();
        }
    }
}
