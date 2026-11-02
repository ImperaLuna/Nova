import QtQuick
import ".."
import "../../volume/service"
import "../../theme/icons.js" as Icons

// microphone.tile: mutes the default source; lit while muted.
QsSlot {
    id: slot

    implicitHeight: tile.implicitHeight

    Tile {
        id: tile
        width: slot.width
        icon: SoundService.sourceMuted ? Icons.MIC_OFF : Icons.MIC
        title: qsTr("Microphone")
        on: SoundService.sourceReady && SoundService.sourceMuted
        available: SoundService.sourceReady
        onToggled: SoundService.toggleSourceMute()
    }
}
