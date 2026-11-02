import QtQuick
import "../theme/icons.js" as Icons
import "bluetooth.js" as Bluetooth

// The Bluetooth tile. A click turns it on when it's off, and opens the page
// when it's on: turning it off is the page's switch, so a click meant for
// the page doesn't drop the headphones. Turned off by a hardware switch, it
// opens the page, which says so. Only properties in and signals out, so
// ui/tests can drive it.
Tile {
    id: tile

    // A bluetooth event's data, or null.
    property var bluetooth: null

    // Turn Bluetooth on (true) or off.
    signal powered(bool on)
    signal pageRequested

    objectName: "bluetoothTile"
    icon: Icons.BLUETOOTH
    title: qsTr("Bluetooth")
    on: Bluetooth.isOn(bluetooth)
    available: (bluetooth?.radio ?? "missing") !== "missing"
    opens: true
    onToggled: bluetooth.radio === "off" ? powered(true) : pageRequested()
    onOpened: pageRequested()
}
