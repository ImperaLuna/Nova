pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "notifications.js" as Notifications
import "../text/duration.js" as Duration

// A pop-up: who it's from, the app, the summary and body (a low battery
// alert's battery and percent instead), progress, and the app's buttons
// (the first one in the accent). A thin line along the bottom drains over
// its time; the pointer on it holds it. Critical ones have a rim in the error color and
// stay until they're closed. A click opens it, ✕ (on hover) dismisses it.
Item {
    id: card

    required property var notification
    property var iconSource: name => ""
    // Seconds since the epoch, for how long ago it came.
    property real now: Date.now() / 1000

    readonly property bool held: hover.hovered
    // Not open yet: its time doesn't run.
    property bool waiting: false
    readonly property int timeout: Notifications.timeout(notification, Tokens.durationNotification)
    readonly property bool critical: notification.urgency === "critical"
    readonly property bool battery: !!notification.device?.battery

    signal clicked
    signal dismissed
    // One of its buttons, by key.
    signal actionClicked(string key)
    // Its time ran out.
    signal expired

    implicitWidth: Tokens.notificationWidth
    implicitHeight: content.implicitHeight + Tokens.pad + Tokens.notificationPadBottom

    readonly property alias radius: surface.radius
    readonly property alias glass: surface

    // Its time starts over (it opened, or its notification came again).
    function restart() {
        timer.restart();
    }

    HoverHandler {
        id: hover
    }

    GlassShadow {
        glass: surface
    }

    Glass {
        id: surface
        anchors.fill: parent
        radius: Tokens.radiusNotification
        clip: true

        // A little lighter under the pointer.
        Rectangle {
            anchors.fill: parent
            radius: parent.radius
            color: Tokens.hover
            visible: card.held
        }

        Rectangle {
            anchors.fill: parent
            radius: parent.radius
            color: "transparent"
            visible: card.critical
            border.width: Tokens.notificationCriticalRim
            border.color: Tokens.error
        }

        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: card.clicked()
        }

        Row {
            id: content
            x: Tokens.pad + Tokens.labelNudge * 2
            y: Tokens.pad
            width: parent.width - 2 * x
            spacing: Tokens.pad

            NotificationIcon {
                notification: card.notification
                iconSource: card.iconSource
            }

            Column {
                width: parent.width - x
                spacing: Tokens.notificationLineGap

                Item {
                    width: parent.width
                    height: Tokens.notificationAppLine

                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width - when.width - Tokens.gap
                        elide: Text.ElideRight
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSizeTiny
                        color: Tokens.fgSoft
                        textFormat: Text.PlainText
                        text: card.notification.app
                    }

                    Text {
                        id: when
                        anchors.right: parent.right
                        anchors.verticalCenter: parent.verticalCenter
                        visible: !card.held
                        font.family: Tokens.fontMono
                        font.pixelSize: Tokens.fontSizeMicro
                        color: Tokens.fgSoft
                        text: Duration.ago(card.notification.time, card.now)
                    }
                }

                // The summary; a low battery alert's battery and percent
                // at its end, in place of the body.
                Item {
                    width: parent.width
                    height: summary.implicitHeight

                    Text {
                        id: summary
                        width: parent.width - (amount.visible ? amount.width + Tokens.gap : 0)
                        elide: Text.ElideRight
                        font.family: Tokens.fontSans
                        font.pixelSize: Tokens.fontSize
                        font.weight: Font.DemiBold
                        color: Tokens.fg
                        textFormat: Text.PlainText
                        text: card.notification.summary
                    }
                    BatteryAmount {
                        id: amount
                        anchors.right: parent.right
                        anchors.verticalCenter: summary.verticalCenter
                        visible: card.battery
                        notification: card.notification
                    }
                }

                Text {
                    width: parent.width
                    visible: !card.battery && text !== ""
                    wrapMode: Text.Wrap
                    maximumLineCount: Tokens.notificationBodyLines
                    elide: Text.ElideRight
                    font.family: Tokens.fontSans
                    font.pixelSize: Tokens.fontSizeSmall
                    color: Tokens.fgDim
                    textFormat: Text.PlainText
                    text: card.notification.body
                }

                Rectangle {
                    visible: card.notification.progress !== undefined
                    width: parent.width
                    height: Tokens.notificationProgressHeight
                    radius: height / 2
                    color: Tokens.track

                    Rectangle {
                        width: parent.width * (card.notification.progress ?? 0) / 100
                        height: parent.height
                        radius: parent.radius
                        color: Tokens.primary

                        Behavior on width {
                            NumberAnimation {
                                duration: Tokens.durationPanel
                                easing.type: Easing.BezierSpline
                                easing.bezierCurve: Tokens.easing
                            }
                        }
                    }
                }

                // Buttons that don't fit on a line wrap to the next.
                Flow {
                    id: actions
                    visible: (card.notification.actions ?? []).length > 0
                    width: parent.width
                    topPadding: Tokens.notificationActionsGap
                    spacing: Tokens.iconGap

                    Repeater {
                        model: card.notification.actions ?? []

                        Rectangle {
                            id: action
                            required property var modelData
                            required property int index
                            readonly property bool primary: index === 0
                            width: Math.min(label.implicitWidth + 2 * Tokens.pad, actions.width)
                            height: Tokens.notificationActionHeight
                            radius: height / 2
                            color: primary ? Tokens.primary : press.containsMouse ? Tokens.selectedQuiet : Tokens.selected

                            Text {
                                id: label
                                anchors.centerIn: parent
                                width: Math.min(implicitWidth, parent.width - 2 * Tokens.pad)
                                elide: Text.ElideRight
                                font.family: Tokens.fontSans
                                font.pixelSize: Tokens.fontSizeSmall
                                font.weight: action.primary ? Font.DemiBold : Font.Medium
                                color: action.primary ? Tokens.onAccent : Tokens.fg
                                textFormat: Text.PlainText
                                text: action.modelData.label
                            }

                            MouseArea {
                                id: press
                                anchors.fill: parent
                                hoverEnabled: true
                                onClicked: card.actionClicked(action.modelData.key)
                            }
                        }
                    }
                }
            }
        }

        RoundButton {
            objectName: "dismiss"
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: Tokens.notificationCloseInset
            visible: card.held
            icon: Icons.CLOSE
            size: Tokens.notificationCloseSize
            onClicked: card.dismissed()
        }

        // The time left.
        Countdown {
            id: timer
            objectName: "timer"
            visible: card.timeout > 0
            x: Tokens.pad + Tokens.labelNudge * 2
            anchors.bottom: parent.bottom
            width: parent.width - 2 * x
            height: Tokens.notificationTimerHeight
            duration: card.timeout
            held: card.held || card.waiting
            onExpired: card.expired()
        }
    }
}
