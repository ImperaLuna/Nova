import QtQuick
import ".."
import "../../volume/service"
import "../../volume/volume.js" as Volume
import "../quick_settings.js" as QuickSettings

// sound.slider: the default output's volume; its arrow opens the sound
// page.
QsSlot {
    id: slot

    readonly property bool available: VolumeService.ready

    implicitHeight: slider.implicitHeight

    QsSlider {
        id: slider
        width: slot.width
        glyph: Volume.icon(slot.available ? VolumeService.output : "none", VolumeService.muted, VolumeService.volume)
        label: slot.available ? VolumeService.outputName : qsTr("No output")
        value: VolumeService.volume
        muted: VolumeService.muted
        available: slot.available
        opens: true
        onMoved: value => VolumeService.slide(value)
        onIconClicked: VolumeService.toggleMute()
        onOpened: slot.qs.pageRequested(QuickSettings.PAGE_SOUND)
    }
}
