import QtQuick
import "network.js" as Network

// The network tile: lit while a link is up or coming up. The whole tile
// opens the network page (its switches are there). Only properties in and
// a signal out, so ui/tests can drive it.
Tile {
    id: tile

    // A network event's data, or null.
    property var network: null

    signal pageRequested

    objectName: "networkTile"
    icon: Network.tileIcon(network)
    iconFill: Network.iconFill(icon)
    title: qsTr("Network")
    on: (network?.primary ?? "") !== "" || Network.connecting(network) !== ""
    opens: true
    onToggled: pageRequested()
    onOpened: pageRequested()
}
