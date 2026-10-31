import QtQuick
import "../quick_settings"
import "../theme"
import "../quick_settings/devices.js" as Devices

// How full a low battery alert's device is, at the end of its title line:
// its battery and its percent, or the level novad words it as (the body)
// when it gives none. The same battery the Devices card draws.
Row {
    id: amount

    required property var notification
    readonly property var battery: notification.device?.battery ?? null

    spacing: Tokens.gap

    DeviceBattery {
        objectName: "deviceBattery"
        anchors.verticalCenter: parent.verticalCenter
        fill: Devices.fill(amount.battery?.percent ?? -1, amount.battery?.level ?? "")
        low: true
    }
    Text {
        objectName: "percent"
        anchors.verticalCenter: parent.verticalCenter
        font.family: Tokens.fontSans
        font.pixelSize: Tokens.fontSizeSmall
        font.weight: Font.DemiBold
        color: Tokens.error
        textFormat: Text.PlainText
        text: amount.battery?.percent !== undefined ? `${amount.battery.percent}%` : amount.notification.body
    }
}
