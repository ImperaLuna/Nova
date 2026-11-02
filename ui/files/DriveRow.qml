import QtQuick
import "../theme"
import "../theme/icons.js" as Icons

// A drive in Nova Files' sidebar: its icon (a USB stick or a disk), its
// name, and under it how full it is while it's mounted (and novad could
// tell). `arrived` slides
// it in and glows it once, for a stick just plugged in. A click opens it,
// a right-click asks for its menu at the pointer (in the window).
Rectangle {
    id: row

    required property var drive
    property bool current: false
    property bool arrived: false

    signal opened
    signal menuRequested(point at)

    readonly property bool mounted: drive.mount !== undefined
    readonly property bool measured: mounted && drive.used !== undefined && drive.size > 0
    readonly property real fill: measured ? Math.min(1, drive.used / drive.size) : 0

    height: Tokens.filesDriveHeight
    radius: Tokens.menuRowRadius
    color: current ? Tokens.selected : hover.hovered ? Tokens.hover : "transparent"

    Behavior on color {
        ColorAnimation {
            duration: Tokens.durationFast
        }
    }

    Rectangle {
        id: glow
        anchors.fill: parent
        radius: parent.radius
        color: Tokens.alpha(Tokens.primary, Tokens.filesDriveGlow)
        opacity: 0
    }

    Row {
        id: content
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Tokens.gap
        anchors.rightMargin: Tokens.gap
        anchors.verticalCenter: parent.verticalCenter
        spacing: Tokens.rowInset

        LineIcon {
            anchors.verticalCenter: parent.verticalCenter
            path: row.drive.removable ? Icons.USB : Icons.DISK
        }
        Column {
            anchors.verticalCenter: parent.verticalCenter
            width: content.width - Tokens.iconSize - content.spacing
            spacing: Tokens.filesDriveBarGap

            Text {
                width: parent.width
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSize
                font.weight: row.current ? Tokens.weightLabel : Tokens.weightBody
                color: Tokens.fg
                elide: Text.ElideRight
                textFormat: Text.PlainText
                text: row.drive.name
            }
            Rectangle {
                visible: row.measured
                width: parent.width
                height: Tokens.filesDriveBarHeight
                radius: height / 2
                color: Tokens.track

                Rectangle {
                    width: parent.width * row.fill
                    height: parent.height
                    radius: parent.radius
                    color: Tokens.meter

                    Behavior on width {
                        NumberAnimation {
                            duration: Tokens.durationFill
                            easing.type: Easing.BezierSpline
                            easing.bezierCurve: Tokens.easing
                        }
                    }
                }
            }
        }
    }

    HoverHandler {
        id: hover
    }
    TapHandler {
        acceptedButtons: Qt.LeftButton
        onTapped: row.opened()
    }
    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: eventPoint => row.menuRequested(row.mapToItem(null, eventPoint.position))
    }

    // Slides in from the left, then glows in the accent and fades back.
    SequentialAnimation {
        running: row.arrived && row.visible
        ParallelAnimation {
            NumberAnimation {
                target: content
                property: "anchors.leftMargin"
                from: Tokens.gap - Tokens.pad
                to: Tokens.gap
                duration: Tokens.durationDriveArrive
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
            NumberAnimation {
                target: row
                property: "opacity"
                from: 0
                to: 1
                duration: Tokens.durationDriveArrive
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Tokens.easing
            }
        }
        NumberAnimation {
            target: glow
            property: "opacity"
            from: 1
            to: 0
            duration: Tokens.durationDriveGlow
            easing.type: Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
    }
}
