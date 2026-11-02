import QtQuick
import ".."
import "../service"
import "../../theme/icons.js" as Icons

// keep-awake.tile: holds off idle (every screen's stage inhibits it while
// on).
QsSlot {
    id: slot

    implicitHeight: tile.implicitHeight

    Tile {
        id: tile
        width: slot.width
        icon: Icons.CUP
        title: qsTr("Keep awake")
        on: QuickSettingsService.keepAwake
        onToggled: QuickSettingsService.keepAwake = !QuickSettingsService.keepAwake
    }
}
