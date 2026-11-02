import QtQuick
import ".."
import "../service"
import "../quick_settings.js" as QuickSettings

// bluetooth.tile: the Bluetooth tile.
QsSlot {
    id: slot

    present: BluetoothService.ready
    implicitHeight: tile.implicitHeight

    BluetoothTile {
        id: tile
        width: slot.width
        bluetooth: BluetoothService.state
        onPowered: on => BluetoothService.setPower(on)
        onPageRequested: slot.qs.pageRequested(QuickSettings.PAGE_BLUETOOTH)
    }
}
