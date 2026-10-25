pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "capture.js" as Capture

// The capture toolbar's switch between a screenshot (the camera, left) and
// a recording (the video, right), like a phone's: the accent knob slides
// under the one that's on. A click anywhere on it asks for the other.
Rectangle {
    id: control

    // The video side is on.
    property bool video: false
    // The knob slides; off, it jumps.
    property bool animated: true
    signal toggled

    readonly property real knobWidth: (width - 2 * Tokens.captureSwitchPad) / 2

    implicitWidth: Tokens.captureSwitchWidth
    implicitHeight: Tokens.captureButtonHeight
    radius: height / 2
    color: input.containsMouse ? Tokens.hover : Tokens.surfaceRaised
    border.width: Tokens.dividerWidth
    border.color: Tokens.outline

    Behavior on color {
        ColorAnimation {
            duration: Tokens.durationColor
        }
    }

    Rectangle {
        id: knob
        x: Tokens.captureSwitchPad + (control.video ? control.knobWidth : 0)
        y: Tokens.captureSwitchPad
        width: control.knobWidth
        height: control.height - 2 * Tokens.captureSwitchPad
        radius: height / 2
        color: Tokens.primary

        Behavior on x {
            enabled: control.animated
            NumberAnimation {
                duration: Tokens.durationPanel
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }
    }

    Repeater {
        model: [
            {
                video: false,
                icon: Capture.ICON_CAMERA
            },
            {
                video: true,
                icon: Capture.ICON_VIDEO
            }
        ]
        CaptureIcon {
            required property var modelData
            x: Tokens.captureSwitchPad + (modelData.video ? control.knobWidth : 0) + (control.knobWidth - width) / 2
            anchors.verticalCenter: parent.verticalCenter
            name: modelData.icon
            color: control.video === modelData.video ? Tokens.onAccent : Tokens.fgSoft

            Behavior on color {
                ColorAnimation {
                    duration: Tokens.durationColor
                }
            }
        }
    }

    MouseArea {
        id: input
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: control.toggled()
    }
}
