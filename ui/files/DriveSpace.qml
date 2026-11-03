import QtQuick
import "../theme"
import "space.js" as Space

// How full a drive is: a bar, and under it what's free ("197 GB free of
// 931 GB"). The bar turns to the warning colour once less than a tenth is
// free. Shown in the sidebar under each drive's name and over a drive's
// "What uses space" list; nothing while novad can't say (not mounted).
Column {
    id: space

    required property var drive
    property int barHeight: Tokens.filesDriveBarHeight
    property int fontSize: Tokens.fontSizeSmall

    readonly property bool low: Space.low(drive)

    visible: Space.measured(drive)
    spacing: Tokens.filesDriveBarGap

    Rectangle {
        width: parent.width
        height: space.barHeight
        radius: height / 2
        color: Tokens.track

        Rectangle {
            width: parent.width * Space.fill(space.drive)
            height: parent.height
            radius: parent.radius
            color: space.low ? Tokens.warning : Tokens.meter

            Behavior on width {
                NumberAnimation {
                    duration: Tokens.durationFill
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Tokens.easing
                }
            }
            Behavior on color {
                ColorAnimation {
                    duration: Tokens.durationFast
                }
            }
        }
    }
    Text {
        width: parent.width
        font.family: Tokens.fontSans
        font.pixelSize: space.fontSize
        color: space.low ? Tokens.warning : Tokens.filesFaint
        elide: Text.ElideRight
        textFormat: Text.PlainText
        text: Space.freeOf(space.drive)
    }
}
