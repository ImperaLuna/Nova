pragma ComponentBehavior: Bound

import QtQuick
import "../notifications"
import "../theme"
import "../theme/icons.js" as Icons
import "../notifications/notifications.js" as Notifications

// Quick settings' notifications, between the sliders and the resources
// card: a heading with how many there are, Clear all, the do not disturb
// bell and the settings gear; then the notifications grouped by app, the
// app with the newest first. An app with two or more is a stack (its
// newest, with the edges of the rest under it and "+N"), which a click
// opens in place, with Show less and Clear. With none, "All caught up".
// Everything comes in as properties and goes out as signals, so ui/tests
// can drive it without novad.
Item {
    id: list

    // A notifications event's items, newest first.
    property var items: []
    property bool doNotDisturb: false
    property var iconSource: name => ""
    property real now: Date.now() / 1000

    signal opened(var notification)
    signal dismissed(int id)
    signal cleared(var app)
    signal muted(string app)
    signal doNotDisturbToggled
    signal settingsOpened

    // The apps whose stacks are open, and the notification whose ⋯ menu is.
    property var expanded: ({})
    property int menuFor: -1

    readonly property var groups: Notifications.groups(items)

    function toggleStack(app: string, open: bool) {
        const next = Object.assign({}, expanded);
        if (open)
            next[app] = true;
        else
            delete next[app];
        expanded = next;
    }

    Item {
        id: heading
        width: parent.width
        height: Tokens.notificationButton

        Row {
            anchors.verticalCenter: parent.verticalCenter
            spacing: Tokens.gap

            CardTitle {
                anchors.verticalCenter: parent.verticalCenter
                text: qsTr("Notifications")
            }
            Rectangle {
                visible: list.items.length > 0
                anchors.verticalCenter: parent.verticalCenter
                width: count.implicitWidth + 2 * Tokens.notificationChipPad
                height: Tokens.notificationChipHeight
                radius: height / 2
                color: Tokens.primary

                Text {
                    id: count
                    anchors.centerIn: parent
                    font.family: Tokens.fontMono
                    font.pixelSize: Tokens.fontSizeMicro
                    color: Tokens.onAccent
                    text: list.items.length
                }
            }
        }

        Row {
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: Tokens.iconGap

            TextButton {
                objectName: "clearAll"
                visible: list.items.length > 0
                anchors.verticalCenter: parent.verticalCenter
                label: qsTr("Clear all")
                onClicked: list.cleared(null)
            }
            RoundButton {
                objectName: "doNotDisturb"
                icon: list.doNotDisturb ? Icons.BELL_OFF : Icons.BELL
                on: list.doNotDisturb
                onClicked: list.doNotDisturbToggled()
            }
            RoundButton {
                objectName: "settings"
                icon: Icons.GEAR
                onClicked: list.settingsOpened()
            }
        }
    }

    Flickable {
        id: scroll
        anchors.top: heading.bottom
        anchors.topMargin: Tokens.gap
        anchors.bottom: parent.bottom
        width: parent.width
        contentHeight: column.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        visible: list.items.length > 0

        Column {
            id: column
            width: scroll.width
            spacing: Tokens.notificationListGap

            Repeater {
                model: list.groups

                Loader {
                    id: group
                    required property var modelData
                    objectName: `group-${modelData.app}`
                    readonly property bool stacked: modelData.items.length > 1
                    readonly property bool open: list.expanded[modelData.app] === true
                    width: column.width
                    // The open menu over the next groups.
                    z: modelData.items.some(item => item.id === list.menuFor) ? 1 : 0
                    sourceComponent: !stacked ? single : open ? openStack : closedStack

                    Component {
                        id: single
                        Entry {
                            notification: group.modelData.items[0]
                        }
                    }

                    Component {
                        id: closedStack
                        Item {
                            implicitHeight: newest.implicitHeight + Tokens.notificationPeek * (group.modelData.items.length > 2 ? 2 : 1)

                            Peek {
                                visible: group.modelData.items.length > 2
                                level: 2
                                above: newest.height
                            }
                            Peek {
                                level: 1
                                above: newest.height
                            }
                            Entry {
                                id: newest
                                notification: group.modelData.items[0]
                                stackOf: group.modelData.app
                            }
                            Rectangle {
                                anchors.right: parent.right
                                anchors.rightMargin: Tokens.pad
                                anchors.bottom: parent.bottom
                                width: rest.implicitWidth + 2 * Tokens.notificationChipPad
                                height: Tokens.notificationChipHeight
                                radius: height / 2
                                color: Tokens.bg
                                border.width: Tokens.dividerWidth
                                border.color: Tokens.glassRim

                                Text {
                                    id: rest
                                    anchors.centerIn: parent
                                    font.family: Tokens.fontMono
                                    font.pixelSize: Tokens.fontSizeMicro
                                    color: Tokens.fg
                                    text: `+${group.modelData.items.length - 1}`
                                }
                            }
                        }
                    }

                    Component {
                        id: openStack
                        Column {
                            spacing: Tokens.notificationListGap

                            Item {
                                width: parent.width
                                height: Tokens.notificationChipHeight + Tokens.labelNudge * 4

                                Row {
                                    x: Tokens.labelNudge * 4
                                    anchors.verticalCenter: parent.verticalCenter
                                    spacing: Tokens.gap

                                    Text {
                                        font.family: Tokens.fontSans
                                        font.pixelSize: Tokens.fontSizeSmall
                                        font.weight: Font.DemiBold
                                        color: Tokens.fg
                                        textFormat: Text.PlainText
                                        text: group.modelData.app
                                    }
                                    Text {
                                        font.family: Tokens.fontMono
                                        font.pixelSize: Tokens.fontSizeTiny
                                        color: Tokens.fgDim
                                        text: group.modelData.items.length
                                    }
                                }
                                Row {
                                    anchors.right: parent.right
                                    anchors.verticalCenter: parent.verticalCenter
                                    TextButton {
                                        objectName: "showLess"
                                        label: qsTr("Show less")
                                        onClicked: list.toggleStack(group.modelData.app, false)
                                    }
                                    TextButton {
                                        objectName: "clearApp"
                                        label: qsTr("Clear")
                                        onClicked: {
                                            list.toggleStack(group.modelData.app, false);
                                            list.cleared(group.modelData.app);
                                        }
                                    }
                                }
                            }

                            Repeater {
                                model: group.modelData.items
                                Entry {
                                    required property var modelData
                                    notification: modelData
                                    showApp: false
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Column {
        objectName: "empty"
        anchors.centerIn: scroll
        visible: list.items.length === 0
        spacing: Tokens.gap

        LineIcon {
            anchors.horizontalCenter: parent.horizontalCenter
            path: Icons.BELL
            size: Tokens.notificationEmptyIcon
            color: Tokens.fgDim
        }
        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            font.family: Tokens.fontDisplay
            font.italic: true
            font.pixelSize: Tokens.fontSizeEmpty
            color: Tokens.fg
            text: qsTr("All caught up")
        }
    }

    // A notification's row, wired to the list.
    component Entry: NotificationRow {
        // What a click does: opens the notification, or (`stackOf`, a
        // closed stack's top one) opens that app's stack in place.
        property string stackOf: ""
        objectName: `row-${notification.id}`
        width: column.width
        iconSource: list.iconSource
        now: list.now
        menuOpen: list.menuFor === notification.id
        onClicked: {
            list.menuFor = -1;
            if (stackOf)
                list.toggleStack(stackOf, true);
            else
                list.opened(notification);
        }
        onMenuToggled: list.menuFor = menuOpen ? -1 : notification.id
        onMuteClicked: {
            list.menuFor = -1;
            list.muted(notification.app);
        }
        onDismissClicked: {
            list.menuFor = -1;
            list.dismissed(notification.id);
        }
    }

    // The edge of a stacked notification under the top one.
    component Peek: Rectangle {
        // 1 right under the top one, 2 under that.
        required property int level
        // The top one's height: each edge shows below it.
        required property real above
        x: level * Tokens.notificationPeekInset
        width: parent.width - 2 * x
        y: above + level * Tokens.notificationPeek - height
        height: Tokens.notificationPeekHeight
        radius: height / 2
        color: level === 1 ? Tokens.selected : Tokens.surfaceRaised
    }

    // A quiet text button, like Clear all.
    component TextButton: Rectangle {
        id: button
        required property string label
        signal clicked

        width: buttonText.implicitWidth + 2 * Tokens.gap
        height: Tokens.notificationChipHeight
        radius: Tokens.radiusSegment
        color: buttonPress.containsMouse ? Tokens.hover : "transparent"

        Text {
            id: buttonText
            anchors.centerIn: parent
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeTiny
            font.weight: Font.Medium
            color: Tokens.fg
            textFormat: Text.PlainText
            text: button.label
        }
        MouseArea {
            id: buttonPress
            anchors.fill: parent
            hoverEnabled: true
            onClicked: button.clicked()
        }
    }
}
