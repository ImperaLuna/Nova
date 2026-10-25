import QtQuick
import "../theme"

// An on/off switch: the accent while on, a quiet track while off, its knob
// sliding across. It only shows; the row it sits in takes the click.
Rectangle {
    id: toggle

    property bool on: false

    implicitWidth: Tokens.switchWidth
    implicitHeight: Tokens.switchHeight
    radius: height / 2
    color: on ? Tokens.primary : Tokens.track

    Behavior on color {
        ColorAnimation {
            duration: Tokens.durationColor
        }
    }

    Rectangle {
        readonly property real inset: (toggle.height - height) / 2
        width: Tokens.switchKnob
        height: width
        radius: width / 2
        y: inset
        x: toggle.on ? toggle.width - width - inset : inset
        color: toggle.on ? Tokens.onAccent : Tokens.fgSoft

        Behavior on x {
            NumberAnimation {
                duration: Tokens.durationColor
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }
    }
}
