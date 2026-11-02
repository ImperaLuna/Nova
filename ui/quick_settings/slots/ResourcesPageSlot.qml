import QtQuick
import ".."
import "../service"
import "../quick_settings.js" as QuickSettings

// resources.page: the busiest apps, searchable, and ending one. Readings
// keep coming while it shows (resources.card asks for them).
QsSlot {
    id: slot

    ResourcesPage {
        anchors.fill: parent
        headerInset: slot.qs.headerInset
        reading: QuickSettingsService.resources
        onBack: slot.qs.pageRequested(QuickSettings.PAGE_MAIN)
        onEndRequested: key => QuickSettingsService.endApp(key)
    }
}
