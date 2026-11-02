import QtQuick
import ".."
import "../service"

// peripherals.card: wireless devices and their batteries, while any is
// connected.
QsSlot {
    id: slot

    present: QuickSettingsService.devices.length > 0
    running: card.running
    implicitHeight: card.implicitHeight

    DevicesCard {
        id: card
        width: slot.width
        devices: QuickSettingsService.devices
    }
}
