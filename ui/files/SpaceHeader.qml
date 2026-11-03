import QtQuick
import "../theme"

// Over the "What uses space" list: the drive the folder is on, by name,
// and how full it is (DriveSpace, thicker than the sidebar's). It opens
// and closes by growing and shrinking, so the list slides under it.
Item {
    id: header

    // The drive, or null.
    required property var drive
    property bool shown: false

    // The drive shown last, kept while it closes.
    property var held: null
    onDriveChanged: {
        if (drive !== null)
            held = drive;
    }

    height: shown ? content.implicitHeight + 2 * Tokens.filesSpacePad : 0
    clip: true
    opacity: shown ? 1 : 0

    Behavior on height {
        NumberAnimation {
            duration: Tokens.durationPanel
            easing.type: Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
    }
    Behavior on opacity {
        NumberAnimation {
            duration: Tokens.durationPanel
            easing.type: Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
    }

    Column {
        id: content
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Tokens.filesSpacePad
        anchors.leftMargin: 2 * Tokens.rowInset
        anchors.rightMargin: 2 * Tokens.rowInset
        spacing: Tokens.filesDriveBarGap
        visible: header.held !== null

        Text {
            width: parent.width
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeHeading
            font.weight: Tokens.weightLabel
            color: Tokens.fg
            elide: Text.ElideRight
            textFormat: Text.PlainText
            text: header.held?.name ?? ""
        }
        DriveSpace {
            width: parent.width
            drive: header.held ?? {}
            barHeight: Tokens.filesSpaceBarHeight
        }
    }
}
