import QtQuick
import ".."
import "../../icons"
import "../../volume/service"
import "../quick_settings.js" as QuickSettings

// sound.page: outputs, inputs and app volumes. SoundService lists them
// only while it's loaded.
QsSlot {
    id: slot

    Component.onCompleted: SoundService.listing = true
    Component.onDestruction: SoundService.listing = false

    SoundPage {
        anchors.fill: parent
        headerInset: slot.qs.headerInset
        outputAvailable: VolumeService.ready
        outputName: VolumeService.outputName
        outputKind: VolumeService.output
        volume: VolumeService.volume
        muted: VolumeService.muted
        inputAvailable: SoundService.sourceReady
        inputName: SoundService.sourceName
        inputVolume: SoundService.sourceVolume
        inputMuted: SoundService.sourceMuted
        onInputVolumeMoved: value => SoundService.setSourceVolume(value)
        onInputMuteToggled: SoundService.toggleSourceMute()
        outputs: SoundService.outputs
        inputs: SoundService.inputs
        apps: SoundService.apps
        onBack: slot.qs.pageRequested(QuickSettings.PAGE_MAIN)
        onVolumeMoved: value => VolumeService.slide(value)
        onMuteToggled: VolumeService.toggleMute()
        onOutputPicked: id => SoundService.useOutput(id)
        onInputPicked: id => SoundService.useInput(id)
        onAppVolumeMoved: (ids, value) => SoundService.setVolumes(ids, value)
        onAppMuteToggled: (ids, muted) => SoundService.setMuted(ids, muted)
        iconSource: name => AppIcons.source(name)
    }
}
