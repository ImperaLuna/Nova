import QtQuick
import "../theme"

// One workspace in the central pill: its number (or name) in a round cell,
// placed by its index so it glides when others come and go, and scaling in
// when it's new. The capsule behind the shown one is the pill's, so it can
// slide; this only picks the number's color, and draws the hover wash and
// the "wants attention" dot.
Item {
    id: number

    // From WorkspaceList.
    required property int index
    required property var workspaceId
    required property string name
    required property bool active
    required property bool urgent

    property bool outputFocused: false
    property bool canFocus: false

    signal clicked

    // Whether the dot shows: a workspace you're on needs no reminder.
    readonly property bool calling: urgent && !active

    x: index * (Tokens.workspaceSize + Tokens.workspaceGap)
    width: Tokens.workspaceSize
    height: parent.height

    Behavior on x {
        NumberAnimation {
            duration: Tokens.durationPanel
            easing.type: Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
    }

    // New numbers grow in; the first ones, there when the pill appears, too.
    scale: Tokens.enterScale
    opacity: 0
    Component.onCompleted: {
        scale = 1;
        opacity = 1;
    }
    Behavior on scale {
        NumberAnimation {
            duration: Tokens.durationPanel
            easing.type: Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
    }
    Behavior on opacity {
        NumberAnimation {
            duration: Tokens.durationFade
        }
    }

    Rectangle {
        anchors.centerIn: parent
        width: Tokens.workspaceSize
        height: Tokens.workspaceSize
        radius: height / 2
        color: area.containsMouse && !(number.active && number.outputFocused) ? Tokens.hover : "transparent"
    }

    Text {
        anchors.centerIn: parent
        width: Tokens.workspaceSize - 2 * Tokens.ringInset
        horizontalAlignment: Text.AlignHCenter
        elide: Text.ElideRight
        font.family: Tokens.fontMono
        font.pixelSize: Tokens.fontSizeSmall
        font.weight: Font.Medium
        color: number.active && number.outputFocused ? Tokens.onAccent : number.calling ? Tokens.primary : Tokens.fg
        textFormat: Text.PlainText
        text: number.name
        Behavior on color {
            ColorAnimation {
                duration: Tokens.durationColor
            }
        }
    }

    // Still, so an urgent workspace costs one short fade, not a frame every
    // vblank for as long as it waits.
    Rectangle {
        id: dot
        objectName: "dot"
        anchors.horizontalCenter: parent.horizontalCenter
        y: (parent.height + Tokens.workspaceSize) / 2 - Tokens.ringInset - height
        width: Tokens.dotSize
        height: Tokens.dotSize
        radius: width / 2
        color: Tokens.primary
        opacity: number.calling ? 1 : 0
        visible: opacity > 0
        Behavior on opacity {
            NumberAnimation {
                duration: Tokens.durationFade
            }
        }
    }

    MouseArea {
        id: area
        anchors.fill: parent
        enabled: number.canFocus
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: number.clicked()
    }
}
