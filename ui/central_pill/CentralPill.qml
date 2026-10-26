pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "../daemon/protocol.js" as Protocol

// The central pill: a search button that opens the launcher (while that
// feature is on), the output's workspace numbers, then the day and the
// time. The bar places it so the time's middle is the screen's middle
// (`timeFromRight`), and it grows to the left as workspaces come and go, so
// the time never moves. The workspace shown on the output sits in a capsule
// that slides between numbers: the accent while the output has focus, a
// quiet fill otherwise. A workspace whose window wants attention gets an
// accent number and a pulsing dot. Clicking a number goes there; scrolling
// over the pill steps through them. Special workspaces (Hyprland's
// scratchpad) lead the numbers, a little apart. Data comes in through its
// properties (the bar binds them to Compositor), so the QML tests can drive
// it.
Item {
    id: pill

    // The output's workspaces to list (Protocol.barWorkspaces): empty when
    // the compositor reports none, and then the pill is only the day and
    // time.
    required property var workspaces
    // Whether this pill's output has the focus.
    property bool outputFocused: false
    // Whether going to a workspace works (Capability focus_workspace).
    property bool canFocus: false
    // Whether the launcher is there to open: the search button shows.
    property bool canSearch: false
    property date now: new Date()

    // A number was clicked: go to that workspace (its id).
    signal focusRequested(var workspaceId)
    // Scrolled: Protocol.STEP_NEXT or STEP_PREVIOUS.
    signal stepRequested(string step)
    // The search button was clicked: open or close the launcher.
    signal searchRequested

    // Drawn as glass (StagePanels), and rounded like it for the mask.
    readonly property bool glass: true
    readonly property real radius: surface.radius
    // How far the time's middle is from the pill's right edge.
    readonly property real timeFromRight: Tokens.pad + time.width / 2

    // Formats: the short day name and the date, and the 24-hour time.
    readonly property string dayFormat: "ddd d"
    readonly property string timeFormat: "hh:mm"
    // One scroll step of a mouse wheel, in angleDelta units (Qt's convention).
    readonly property int wheelStep: 120

    readonly property real step: Tokens.workspaceSize + Tokens.workspaceGap
    // The numbers' padding: as far from the pill's edge as from its top.
    readonly property real numbersInset: (Tokens.barHeight - Tokens.workspaceSize) / 2
    // The gap after the special workspaces, when there are any and numbers
    // after them.
    readonly property real groupGap: list.specialCount > 0 && list.specialCount < list.count ? Tokens.workspaceGroupGap : 0
    readonly property real numbersWidth: list.count > 0 ? list.count * step - Tokens.workspaceGap + groupGap : 0
    // The search button and the divider after it, from the pill's left
    // edge: as far in as the numbers would be, and a gap either side of the
    // divider.
    readonly property real searchRoom: canSearch ? numbersInset + Tokens.workspaceSize + 2 * Tokens.gap + Tokens.dividerWidth : 0
    // The room the numbers and the divider after them take, animated so the
    // pill's left edge glides as numbers come and go.
    // Around the numbers: the gaps either side of the divider and the
    // divider, and without the search button before them, their inset less
    // the pad the pill has anyway.
    readonly property real numbersFrame: 2 * Tokens.gap + Tokens.dividerWidth + (canSearch ? 0 : numbersInset - Tokens.pad)
    property real numbersRoom: list.count > 0 ? numbersWidth + numbersFrame : 0
    Behavior on numbersRoom {
        NumberAnimation {
            duration: Tokens.durationPanel
            easing.type: Easing.BezierSpline
            easing.bezierCurve: Tokens.easing
        }
    }

    implicitWidth: (canSearch ? searchRoom : Tokens.pad) + numbersRoom + day.implicitWidth + Tokens.gap + time.width + Tokens.pad
    implicitHeight: Tokens.barHeight

    onWorkspacesChanged: list.sync(workspaces)
    Component.onCompleted: list.sync(workspaces)

    WorkspaceList {
        id: list
    }

    GlassShadow {
        glass: surface
    }

    Glass {
        id: surface
        anchors.fill: parent
        radius: height / 2
        clip: true

        // Scrolling anywhere over the pill steps through the workspaces, one
        // per wheel notch; touchpads add up their small steps.
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.NoButton
            enabled: pill.canFocus
            property real scrolled: 0
            onWheel: wheel => {
                scrolled += wheel.angleDelta.y || wheel.angleDelta.x;
                while (Math.abs(scrolled) >= pill.wheelStep) {
                    const down = scrolled < 0;
                    pill.stepRequested(down ? Protocol.STEP_NEXT : Protocol.STEP_PREVIOUS);
                    scrolled += down ? pill.wheelStep : -pill.wheelStep;
                }
            }
        }

        Rectangle {
            id: search
            objectName: "search"
            visible: pill.canSearch
            x: pill.numbersInset
            anchors.verticalCenter: parent.verticalCenter
            width: Tokens.workspaceSize
            height: Tokens.workspaceSize
            radius: height / 2
            color: searchMouse.containsMouse ? Tokens.hover : "transparent"
            Behavior on color {
                ColorAnimation {
                    duration: Tokens.durationFast
                }
            }

            LineIcon {
                anchors.centerIn: parent
                size: Tokens.iconSizeSmall
                path: Icons.SEARCH
                color: searchMouse.containsMouse ? Tokens.fg : Tokens.fgDim
            }

            MouseArea {
                id: searchMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: pill.searchRequested()
            }
        }

        Rectangle {
            visible: pill.canSearch
            x: search.x + search.width + Tokens.gap
            anchors.verticalCenter: parent.verticalCenter
            width: Tokens.dividerWidth
            height: Tokens.dividerHeight
            color: Tokens.glassRim
        }

        // The numbers, laid out from the divider leftwards.
        Item {
            id: numbers
            objectName: "numbers"
            // Its right edge stays by the divider; its left edge follows the
            // animated room, so the numbers already there glide over.
            x: divider.x - Tokens.gap - (pill.numbersRoom - pill.numbersFrame)
            width: pill.numbersWidth
            height: parent.height

            Rectangle {
                id: capsule
                objectName: "capsule"
                y: (parent.height - height) / 2
                width: Tokens.workspaceSize
                height: Tokens.workspaceSize
                radius: height / 2
                x: list.activeIndex * pill.step + (list.activeIndex >= list.specialCount ? pill.groupGap : 0)
                visible: list.activeIndex >= 0
                color: pill.outputFocused ? Tokens.primary : Tokens.selectedQuiet
                Behavior on x {
                    NumberAnimation {
                        duration: Tokens.durationPanel
                        easing.type: Easing.BezierSpline
                        easing.bezierCurve: Tokens.easing
                    }
                }
                Behavior on color {
                    ColorAnimation {
                        duration: Tokens.durationColor
                    }
                }
            }

            Repeater {
                model: list

                delegate: WorkspaceNumber {
                    outputFocused: pill.outputFocused
                    shift: special ? 0 : pill.groupGap
                    canFocus: pill.canFocus
                    onClicked: pill.focusRequested(workspaceId)
                }
            }
        }

        Rectangle {
            id: divider
            objectName: "divider"
            x: day.x - Tokens.gap - width
            anchors.verticalCenter: parent.verticalCenter
            width: Tokens.dividerWidth
            height: Tokens.dividerHeight
            color: Tokens.glassRim
            opacity: list.count > 0 ? 1 : 0
        }

        Text {
            id: day
            x: time.x - Tokens.gap - width
            anchors.verticalCenter: parent.verticalCenter
            font.family: Tokens.fontMono
            font.pixelSize: Tokens.fontSize
            font.weight: Font.Medium
            color: Tokens.fgDim
            textFormat: Text.PlainText
            text: Qt.formatDate(pill.now, pill.dayFormat)
        }

        Row {
            id: time
            objectName: "time"
            x: parent.width - Tokens.pad - width
            anchors.verticalCenter: parent.verticalCenter

            readonly property string text: Qt.formatTime(pill.now, pill.timeFormat)

            Repeater {
                model: time.text.length

                delegate: Item {
                    id: slot
                    required property int index
                    readonly property string character: time.text[index]
                    readonly property bool isDigit: character >= "0" && character <= "9"
                    implicitWidth: isDigit ? rolling.implicitWidth : still.implicitWidth
                    implicitHeight: rolling.implicitHeight

                    RollingDigit {
                        id: rolling
                        visible: slot.isDigit
                        text: slot.character
                        font.family: Tokens.fontMono
                        font.pixelSize: Tokens.fontSizeTime
                        font.weight: Font.DemiBold
                        color: Tokens.fg
                    }

                    // The colon doesn't roll.
                    Text {
                        id: still
                        visible: !slot.isDigit
                        anchors.verticalCenter: parent.verticalCenter
                        font: rolling.font
                        color: Tokens.fg
                        textFormat: Text.PlainText
                        text: slot.character
                    }
                }
            }
        }
    }
}
