import QtQuick
import "../launcher"
import "../quick_settings"
import "../theme"
import "../quick_settings/devices.js" as Devices
import "../text/files.js" as Files

// Who a notification is from, `size` square: for Nova's low battery
// alerts, the device's glyph with its low battery on the corner; else the
// picture it came with (a sender's photo), round, on its own; or else the
// app's icon, which falls back to its letter (AppIcon).
Item {
    id: icon

    required property var notification
    // NotificationsService.iconSource, or a stand-in in tests.
    property var iconSource: name => ""
    property int size: Tokens.notificationAvatar

    readonly property var device: notification.device ?? null
    readonly property bool pictured: !device && picture.status === Image.Ready

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
        visible: !icon.device && !icon.pictured
        size: icon.size
        appId: icon.notification.app_id ?? icon.notification.app
        name: icon.notification.app
        source: icon.iconSource(icon.notification.icon ?? "")
    }

    // A low battery alert's device.
    Rectangle {
        objectName: "device"
        anchors.fill: parent
        visible: icon.device !== null
        radius: Math.round(icon.size * Tokens.notificationDeviceRadius)
        color: Tokens.surfaceRaised

        LineIcon {
            anchors.centerIn: parent
            path: Devices.icon(icon.device?.kind ?? "")
            size: Math.round(icon.size * Tokens.notificationDeviceGlyph)
        }
    }

    Rectangle {
        readonly property real scaled: Tokens.notificationDeviceBattery * icon.size / Tokens.notificationAvatar

        visible: icon.device !== null
        x: icon.size + Tokens.notificationDeviceChipOut - width
        y: icon.size + Tokens.notificationDeviceChipOut - height
        width: battery.implicitWidth * scaled + 2 * Tokens.notificationDeviceChipPad
        height: battery.implicitHeight * scaled + 2 * Tokens.notificationDeviceChipPad
        radius: height / 2
        color: Tokens.bg

        DeviceBattery {
            id: battery
            objectName: "deviceBattery"
            anchors.centerIn: parent
            scale: parent.scaled
            fill: Devices.fill(icon.device?.battery?.percent ?? -1, icon.device?.battery?.level ?? "")
            low: true
        }
    }
}
