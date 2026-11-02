import QtQuick
import ".."
import "../service"
import "../quick_settings.js" as QuickSettings

// resources.card: CPU, memory, GPU and temperature rings at the foot. It
// stays loaded while the resources page shows, so it also keeps novad
// reading them for that page.
QsSlot {
    id: slot

    readonly property bool watching: qs.open && (qs.page === QuickSettings.PAGE_MAIN || qs.page === QuickSettings.PAGE_RESOURCES)
    onWatchingChanged: QuickSettingsService.watching = watching
    Component.onCompleted: {
        if (watching)
            QuickSettingsService.watching = true;
    }
    // Unloaded while it watched (its screen went away).
    Component.onDestruction: {
        if (watching)
            QuickSettingsService.watching = false;
    }

    implicitHeight: card.implicitHeight

    ResourcesCard {
        id: card
        width: slot.width
        reading: QuickSettingsService.resources
        fill: slot.qs.fill
        onOpened: slot.qs.pageRequested(QuickSettings.PAGE_RESOURCES)
    }
}
