pragma ComponentBehavior: Bound

import QtQuick
import "../theme"
import "../theme/icons.js" as Icons
import "notifications.js" as Notifications

// A notification in quick settings' list: its icon, "App · summary", how
// long ago, and the body in two lines; a dot while it's unread. A click
// opens it; ⋯ (on hover) offers muting its app and dismissing it.
Item {
    id: row

    required property var notification
    property var iconSource: name => ""
    property real now: Date.now() / 1000
    // The ⋯ menu is open (the list keeps one open at a time).
    property bool menuOpen: false

    signal clicked
    signal menuToggled
    signal muteClicked
    signal dismissClicked

    implicitHeight: Math.max(Tokens.notificationRowIcon, lines.implicitHeight) + 2 * Tokens.rowInset

    Rectangle {
        id: back
        anchors.fill: parent
        radius: Tokens.radiusCard
        color: hover.hovered ? Tokens.hover : Tokens.surfaceRaised
    }

    HoverHandler {
        id: hover
    }

    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: row.clicked()
    }

    Rectangle {
        visible: row.notification.unread === true
        x: (Tokens.pad - width) / 2
        anchors.verticalCenter: parent.verticalCenter
        width: Tokens.dotSize
        height: width
        radius: width / 2
        color: Tokens.primary
    }

    NotificationIcon {
        id: icon
        x: Tokens.pad
        y: Tokens.rowInset
        size: Tokens.notificationRowIcon
        notification: row.notification
        iconSource: row.iconSource
    }

    Column {
        id: lines
        x: icon.x + icon.width + Tokens.rowInset
        y: Tokens.rowInset
        width: parent.width - x - Tokens.pad

        Item {
            width: parent.width
            height: title.implicitHeight

            Text {
                id: title
                width: parent.width - when.width - Tokens.gap
                elide: Text.ElideRight
                font.family: Tokens.fontSans
                font.pixelSize: Tokens.fontSizeSmall
                font.weight: Font.DemiBold
                color: Tokens.fg
                textFormat: Text.PlainText
                text: row.notification.summary === "" ? row.notification.app : `${row.notification.app} · ${row.notification.summary}`
            }
            Text {
                id: when
                anchors.right: parent.right
                visible: !hover.hovered && !row.menuOpen
                font.family: Tokens.fontMono
                font.pixelSize: Tokens.fontSizeTiny
                color: Tokens.fgSoft
                text: Notifications.ago(row.notification.time, row.now)
            }
        }
        Text {
            width: parent.width
            visible: text !== ""
            wrapMode: Text.Wrap
            maximumLineCount: Tokens.notificationRowLines
            elide: Text.ElideRight
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeSmall
            color: Tokens.fgAlt
            textFormat: Text.PlainText
            text: row.notification.body
        }
    }

    RoundButton {
        id: more
        objectName: "more"
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Tokens.notificationCloseInset
        visible: hover.hovered || row.menuOpen
        quiet: true
        size: Tokens.notificationCloseSize
        icon: Icons.DOTS
        fill: Icons.DOTS
        onClicked: row.menuToggled()
    }

    // Over what's below it in the list: the row raises itself meanwhile.
    z: menuOpen ? 1 : 0
    Rectangle {
        objectName: "menu"
        visible: row.menuOpen
        anchors.right: parent.right
        anchors.rightMargin: Tokens.notificationCloseInset
        y: more.y + more.height + Tokens.labelNudge * 2
        width: Tokens.notificationMenuWidth
        height: menu.implicitHeight + 2 * Tokens.segmentPad
        radius: Tokens.radiusControl
        color: Tokens.bg
        border.width: Tokens.dividerWidth
        border.color: Tokens.glassRim

        Column {
            id: menu
            x: Tokens.segmentPad
            y: Tokens.segmentPad
            width: parent.width - 2 * x

            MenuItem {
                objectName: "mute"
                icon: Icons.BELL_OFF
                label: qsTr("Mute %1").arg(row.notification.app)
                onClicked: row.muteClicked()
            }
            MenuItem {
                objectName: "dismiss"
                icon: Icons.CLOSE
                label: qsTr("Dismiss")
                onClicked: row.dismissClicked()
            }
        }
    }

    component MenuItem: Rectangle {
        id: item
        required property string icon
        required property string label
        signal clicked

        width: parent.width
        height: Tokens.segmentHeight
        radius: Tokens.radiusSegment
        color: itemPress.containsMouse ? Tokens.hover : "transparent"

        LineIcon {
            id: itemIcon
            x: Tokens.rowInset
            anchors.verticalCenter: parent.verticalCenter
            path: item.icon
            size: Tokens.iconSizeSmall
        }
        Text {
            x: itemIcon.x + itemIcon.width + Tokens.gap
            width: parent.width - x - Tokens.rowInset
            anchors.verticalCenter: parent.verticalCenter
            elide: Text.ElideRight
            font.family: Tokens.fontSans
            font.pixelSize: Tokens.fontSizeSmall
            color: Tokens.fg
            textFormat: Text.PlainText
            text: item.label
        }
        MouseArea {
            id: itemPress
            anchors.fill: parent
            hoverEnabled: true
            onClicked: item.clicked()
        }
    }
}
