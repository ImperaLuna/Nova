import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol

// One workspace in the central pill: its number (or name) in a round cell,
// placed by its index so it glides when others come and go, and scaling in
// when it's new. The capsule behind the shown one is the pill's, so it can
// slide; this only picks the number's color, and draws the hover wash and
// the "wants attention" dot. A special workspace (Hyprland's scratchpad)
// shows a dim tilde, or its letter, brighter while it shows on another
// output; clicking it shows or hides it.
Item {
    id: number

    // From WorkspaceList.
    required property int index
    required property var workspaceId
    required property string name
    required property bool active
    required property bool urgent
    required property bool special
    required property bool elsewhere

    property bool outputFocused: false
    // Extra room before it: the special workspaces' gap, for the numbers
    // after them.
    property real shift: 0
    property bool canFocus: false

    signal clicked

    // Whether the dot shows: a workspace you're on needs no reminder, and a
    // special one's windows wait there anyway.
    readonly property bool calling: urgent && !active && !special
    readonly property color ink: active && outputFocused ? Tokens.onAccent : calling ? Tokens.primary : special && !active && !elsewhere ? Tokens.fgDim : Tokens.fg

    x: index * (Tokens.workspaceSize + Tokens.workspaceGap) + shift
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

    LineIcon {
        anchors.centerIn: parent
        visible: number.special && number.name === Protocol.SPECIAL_MARK
        size: Tokens.iconSizeSmall
        path: Icons.TILDE
        color: number.ink
        Behavior on color {
            ColorAnimation {
                duration: Tokens.durationColor
            }
        }
    }

    Text {
        anchors.centerIn: parent
        visible: !(number.special && number.name === Protocol.SPECIAL_MARK)
        width: Tokens.workspaceSize - 2 * Tokens.ringInset
        horizontalAlignment: Text.AlignHCenter
        elide: Text.ElideRight
        font.family: Tokens.fontMono
        font.pixelSize: Tokens.fontSizeSmall
        font.weight: Font.Medium
        color: number.ink
        textFormat: Text.PlainText
        text: number.name
        Behavior on color {
            ColorAnimation {
                duration: Tokens.durationColor
            }
        }
    }

    Rectangle {
        id: dot
        anchors.horizontalCenter: parent.horizontalCenter
        y: (parent.height + Tokens.workspaceSize) / 2 - Tokens.ringInset - height
        width: Tokens.dotSize
        height: Tokens.dotSize
        radius: width / 2
        color: Tokens.primary
        visible: number.calling

        SequentialAnimation on opacity {
            running: dot.visible
            loops: Animation.Infinite
            alwaysRunToEnd: true
            NumberAnimation {
                to: Tokens.dimOpacity
                duration: Tokens.durationPulse / 2
                easing.type: Tokens.easingPulse
            }
            NumberAnimation {
                to: 1
                duration: Tokens.durationPulse / 2
                easing.type: Tokens.easingPulse
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
