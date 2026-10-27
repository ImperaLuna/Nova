import QtQuick
import "../launcher"
import "../theme"
import "../text/files.js" as Files

// Who a notification is from, `size` square: the picture it came with (a
// sender's photo), round, on its own; or else the app's icon, which falls
// back to its letter (AppIcon).
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

    // The app's icon, when there's no picture.
    AppIcon {
        anchors.centerIn: parent
        visible: !icon.pictured
        size: icon.size
        appId: icon.notification.app_id ?? icon.notification.app
        name: icon.notification.app
        source: icon.iconSource(icon.notification.icon ?? "")
    }
}
