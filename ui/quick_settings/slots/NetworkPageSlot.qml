import QtQuick
import ".."
import "../service"
import "../quick_settings.js" as QuickSettings

// network.page: links, Wi-Fi networks and traffic. Loaded, it scans and
// watches traffic every second.
QsSlot {
    id: slot

    Component.onCompleted: {
        NetworkService.watching = true;
        NetworkService.scan();
    }
    Component.onDestruction: NetworkService.watching = false

    NetworkPage {
        anchors.fill: parent
        headerInset: slot.qs.headerInset
        network: NetworkService.network
        traffic: NetworkService.traffic
        onBack: slot.qs.pageRequested(QuickSettings.PAGE_MAIN)
        onWifiToggled: enabled => NetworkService.setWifi(enabled)
        onConnectRequested: (ssid, password) => NetworkService.connect(ssid, password)
        onDisconnectRequested: NetworkService.disconnect()
        onForgetRequested: ssid => NetworkService.forget(ssid)
        onPreferPicked: kind => NetworkService.prefer(kind)
    }
}
