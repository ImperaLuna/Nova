import QtQuick
import ".."
import "../service"
import "../quick_settings.js" as QuickSettings

// network.tile: the network tile, once novad has sent the network's state.
QsSlot {
    id: slot

    present: NetworkService.ready
    implicitHeight: tile.implicitHeight

    NetworkTile {
        id: tile
        width: slot.width
        network: NetworkService.network
        onPageRequested: slot.qs.pageRequested(QuickSettings.PAGE_NETWORK)
    }
}
