import QtQuick
import ".."
import "../../theme"
import "../../volume/service"
import "../../theme/icons.js" as Icons
import "../quick_settings.js" as QuickSettings

// microphone.slider: the mic's volume, only while an app records from it,
// named after those apps.
QsSlot {
    id: slot

    present: SoundService.recorders.length > 0
    implicitHeight: slider.implicitHeight

    QsSlider {
        id: slider
        width: slot.width
        icon: SoundService.sourceMuted ? Icons.MIC_OFF : Icons.MIC
        iconColor: Tokens.secondary
        fill: Tokens.alpha(Tokens.secondary, Tokens.micFillOpacity)
        label: QuickSettings.recordingText(SoundService.recorders)
        value: SoundService.sourceVolume
        muted: SoundService.sourceMuted
        onMoved: value => SoundService.setSourceVolume(value)
        onIconClicked: SoundService.toggleSourceMute()
    }
}
