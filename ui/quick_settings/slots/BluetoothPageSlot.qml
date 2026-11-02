import QtQuick
import ".."
import "../service"
import "../quick_settings.js" as QuickSettings

// bluetooth.page: devices, pairing and the radio's switch. While it shows,
// novad looks for devices and asks pairing questions here instead of in
// the pop-up.
QsSlot {
    id: slot

    readonly property bool watching: qs.open && qs.page === QuickSettings.PAGE_BLUETOOTH
    onWatchingChanged: BluetoothService.watching = watching
    Component.onCompleted: {
        if (watching)
            BluetoothService.watching = true;
    }
    // Unloaded while it watched (its screen went away).
    Component.onDestruction: {
        if (watching)
            BluetoothService.watching = false;
    }

    BluetoothPage {
        anchors.fill: parent
        headerInset: slot.qs.headerInset
        bluetooth: BluetoothService.state
        onBack: slot.qs.pageRequested(QuickSettings.PAGE_MAIN)
        onPowerToggled: on => BluetoothService.setPower(on)
        onConnectRequested: address => BluetoothService.connectDevice(address)
        onDisconnectRequested: address => BluetoothService.disconnectDevice(address)
        onForgetRequested: address => BluetoothService.forget(address)
        onAnswered: (id, answer) => BluetoothService.answer(id, answer)
    }
}
