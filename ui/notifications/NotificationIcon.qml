import QtQuick
import "../launcher"
import "../theme"
import "../text/files.js" as Files

// Who a notification is from, `size` square: the picture it came with (a
// sender's photo), round, with the app's icon on its corner; or the app's
// icon alone. The app's icon falls back to its letter (AppIcon).
Item {
    id: icon

    required property var notification
    // NotificationsService.iconSource, or a stand-in in tests.
    property var iconSource: name => ""
    property int size: Tokens.notificationAvatar

    readonly property bool pictured: picture.status === Image.Ready

    implicitWidth: size
    implicitHeight: size

    Rectangle {
        id: round
        anchors.fill: parent
        radius: width / 2
        color: Tokens.surfaceRaised
        visible: icon.pictured
        clip: true

        Image {
            id: picture
            anchors.fill: parent
            source: icon.notification.image ? Files.fileUrl(icon.notification.image) : ""
            sourceSize: Qt.size(icon.size * Screen.devicePixelRatio, icon.size * Screen.devicePixelRatio)
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
            smooth: true
        }
    }

    // The app's icon: on the picture's corner, or the whole icon.
    Rectangle {
        readonly property int badge: Tokens.notificationAppBadge
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: icon.pictured ? -Tokens.notificationBadgeRing : 0
        width: icon.pictured ? badge + 2 * Tokens.notificationBadgeRing : icon.size
        height: width
        radius: icon.pictured ? Tokens.notificationBadgeRadius + Tokens.notificationBadgeRing : 0
        color: icon.pictured ? Tokens.bg : "transparent"

        AppIcon {
            anchors.centerIn: parent
            size: icon.pictured ? parent.badge : icon.size
            appId: icon.notification.app_id ?? icon.notification.app
            name: icon.notification.app
            source: icon.iconSource(icon.notification.icon ?? "")
        }
    }
}
